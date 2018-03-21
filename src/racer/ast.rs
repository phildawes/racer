use core::{self, Match, MatchType, Scope, Ty, Session, SessionExt, Point, SourceByteRange};
use typeinf;
use nameres::{self, resolve_path_with_str};
use scopes;

use std::path::Path;
use std::rc::Rc;

use syntex_errors::Handler;
use syntex_errors::emitter::ColorConfig;
use syntex_syntax::ast::{self, ExprKind, FunctionRetTy, Generics, ItemKind,
                         LitKind, PatKind, TyKind, TyParamBound, TyParamBounds};
use syntex_syntax::codemap;
use syntex_syntax::parse::parser::Parser;
use syntex_syntax::parse::{lexer, ParseSess};
use syntex_syntax::print::pprust;
use syntex_syntax::visit::{self};

// This code ripped from libsyntax::util::parser_testing
pub fn string_to_parser(ps: &ParseSess, source_str: String) -> Option<Parser> {
    let fm = ps.codemap().new_filemap("bogofile".into(), None, source_str);
    let srdr = lexer::StringReader::new(&ps.span_diagnostic, fm);
    let p = Parser::new(ps, Box::new(srdr));
    Some(p)
}

pub fn with_error_checking_parse<F, T>(s: String, f: F) -> Option<T>
    where F: Fn(&mut Parser) -> Option<T>
{
    let cm = Rc::new(codemap::CodeMap::new());
    let sh = Handler::with_tty_emitter(ColorConfig::Never, false, false, Some(cm.clone()));
    let ps = ParseSess::with_span_handler(sh, cm);

    let mut p = match string_to_parser(&ps, s) {
        Some(p) => p,
        None => return None
    };
    f(&mut p)
}

// parse a string, return a stmt
pub fn string_to_stmt(source_str: String) -> Option<ast::Stmt> {
    with_error_checking_parse(source_str, |p| {
        match p.parse_stmt() {
            Ok(Some(stmt)) => Some(stmt),
            _ => None,
        }
    })
}

// parse a string, return a crate.
pub fn string_to_crate(source_str: String) -> Option<ast::Crate> {
    with_error_checking_parse(source_str.clone(), |p| {
        use std::result::Result::{Ok, Err};
        match p.parse_crate_mod() {
            Ok(e) => Some(e),
            Err(mut err) => {
                err.cancel();
                debug!("unable to parse crate. Returning None |{}|", source_str);
                None
            }
        }
    })
}

/// The leaf of a `use` statement. Extends `core::Path` to support
/// aliases in `ViewPathList` scenarios.
#[derive(Debug)]
pub struct PathWithAlias {
    /// The identifier introduced into the current scope.
    pub ident: String,
    /// The path.
    pub path: core::Path,
}

impl From<core::Path> for PathWithAlias {
    fn from(v: core::Path) -> Self {
        PathWithAlias {
            ident: v.segments[0].name.clone(),
            path: v,
        }
    }
}

impl AsRef<core::Path> for PathWithAlias {
    fn as_ref(&self) -> &core::Path {
        &self.path
    }
}

#[derive(Debug)]
pub struct UseVisitor {
    /// For a single `use` statement, contains the identifier introduced into the current
    /// scope.
    ///
    /// * A simple use statement such as `use foo::bar` will set this to `"bar"`.
    /// * An alias such as `use foo::bar as baz` will set this to `"baz"`.
    pub ident : Option<String>,
    pub paths : Vec<PathWithAlias>,
    pub is_glob: bool
}

impl visit::Visitor for UseVisitor {
    fn visit_item(&mut self, i: &ast::Item) {
        if let ItemKind::Use(ref path) = i.node {
            match path.node {
                ast::ViewPathSimple(ident, ref path) => {
                    self.paths.push(PathWithAlias {
                        ident: ident.name.to_string(),
                        path: to_racer_path(path),
                    });
                    self.ident = Some(ident.name.to_string());
                },
                ast::ViewPathList(ref pth, ref paths) => {
                    let basepath = to_racer_path(pth);
                    for path in paths {
                        // Figure out the identifier being introduced to the local
                        // namespace. This will differ from the import name if an `as`
                        // was used.
                        let ident = path.node.rename.unwrap_or(path.node.name).name.to_string();

                        let name = path.node.name.name.to_string();

                        let seg = core::PathSegment{ name: name, types: Vec::new() };
                        let mut newpath = basepath.clone();

                        newpath.segments.push(seg);
                        self.paths.push(PathWithAlias {
                            ident: ident,
                            path: newpath,
                        });
                    }
                }
                ast::ViewPathGlob(ref pth) => {
                    self.paths.push(to_racer_path(pth).into());
                    self.is_glob = true;
                }
            }
        }
    }
}

pub struct PatBindVisitor {
    ident_points: Vec<SourceByteRange>
}

impl visit::Visitor for PatBindVisitor {
    fn visit_local(&mut self, local: &ast::Local) {
        // don't visit the RHS (init) side of the let stmt
        self.visit_pat(&local.pat);
    }

    fn visit_expr(&mut self, ex: &ast::Expr) {
        // don't visit the RHS or block of an 'if let' or 'for' stmt
        if let ExprKind::IfLet(ref pattern, _,_,_) = ex.node {
            self.visit_pat(pattern);
        } else if let ExprKind::WhileLet(ref pattern, _,_,_) = ex.node {
            self.visit_pat(pattern);
        } else if let ExprKind::ForLoop(ref pattern, _, _, _) = ex.node {
            self.visit_pat(pattern);
        } else {
            visit::walk_expr(self, ex)
        }
    }

    fn visit_pat(&mut self, p: &ast::Pat) {
        match p.node {
            PatKind::Ident(_ , ref spannedident, _) => {
                let codemap::BytePos(lo) = spannedident.span.lo;
                let codemap::BytePos(hi) = spannedident.span.hi;
                self.ident_points.push((lo as usize, hi as usize));
            }
            _ => {
                visit::walk_pat(self, p);
            }
        }
    }
}

pub struct PatVisitor {
    ident_points: Vec<SourceByteRange>
}

impl visit::Visitor for PatVisitor {
    fn visit_pat(&mut self, p: &ast::Pat) {
        match p.node {
            PatKind::Ident(_ , ref spannedident, _) => {
                let codemap::BytePos(lo) = spannedident.span.lo;
                let codemap::BytePos(hi) = spannedident.span.hi;
                self.ident_points.push((lo as usize, hi as usize));
            }
            _ => { visit::walk_pat(self, p); }
        }
    }
}

fn to_racer_ty(ty: &ast::Ty, scope: &Scope) -> Option<Ty> {
    match ty.node {
        TyKind::Tup(ref items) => {
            let mut res = Vec::new();
            for t in items {
                res.push(match to_racer_ty(t, scope) {
                    Some(t) => t,
                    None => return None
                });
            }
            Some(Ty::Tuple(res))
        },
        TyKind::Rptr(ref _lifetime, ref ty) => {
            to_racer_ty(&ty.ty, scope).map(|ref_ty| Ty::RefPtr(Box::new(ref_ty)) )
        }
        TyKind::Path(_, ref path) => {
            Some(Ty::PathSearch(to_racer_path(path), scope.clone()))
        }
        TyKind::Array(ref ty, ref expr) => {
            to_racer_ty(ty, scope).map(|racer_ty| {
                Ty::FixedLengthVec(Box::new(racer_ty), pprust::expr_to_string(expr))
            })
        }
        TyKind::Slice(ref ty) => {
            to_racer_ty(ty, scope).map(|ref_ty| Ty::Vec(Box::new(ref_ty)) )
        }
        TyKind::Never => {
            None
        }
        _ => {
            trace!("unhandled Ty node: {:?}", ty.node);
            None
        }
    }
}

fn point_is_in_span(point: u32, span: &codemap::Span) -> bool {
    let codemap::BytePos(lo) = span.lo;
    let codemap::BytePos(hi) = span.hi;
    point >= lo && point < hi
}

// The point must point to an ident within the pattern.
fn destructure_pattern_to_ty(pat: &ast::Pat,
                             point: Point,
                             ty: &Ty,
                             scope: &Scope,
                             session: &Session) -> Option<Ty> {
    debug!("destructure_pattern_to_ty point {} ty {:?}    ||||||||    pat: {:?}", point, ty, pat);
    match pat.node {
        PatKind::Ident(_ , ref spannedident, _) => {
            if point_is_in_span(point as u32, &spannedident.span) {
                debug!("destructure_pattern_to_ty matched an ident!");
                Some(ty.clone())
            } else {
                panic!("Expecting the point to be in the patident span. pt: {}", point);
            }
        }
        PatKind::Tuple(ref tuple_elements, _) => {
            match *ty {
                Ty::Tuple(ref typeelems) => {
                    let mut res = None;
                    for (i, p) in tuple_elements.iter().enumerate() {
                        if point_is_in_span(point as u32, &p.span) {
                            let ty = &typeelems[i];
                            res = destructure_pattern_to_ty(p, point, ty, scope, session);
                            break;
                        }
                    }
                    res
                }
                _ => panic!("Expecting TyTuple")

            }
        }
        PatKind::TupleStruct(ref path, ref children, _) => {
            let m = resolve_ast_path(path, &scope.filepath, scope.point, session);
            let contextty = path_to_match(ty.clone(), session);
            if let Some(m) = m {
                let mut res = None;

                for (i, p) in children.iter().enumerate() {
                    if point_is_in_span(point as u32, &p.span) {

                        res = typeinf::get_tuplestruct_field_type(i, &m, session)
                            .and_then(|ty|
                                // if context ty is a match, use its generics
                                if let Some(Ty::Match(ref contextmatch)) = contextty {
                                    path_to_match_including_generics(ty, contextmatch, session)
                                } else {
                                    path_to_match(ty, session)
                                })
                            .and_then(|ty| destructure_pattern_to_ty(p, point, &ty, scope, session));

                        break;
                    }
                }
                res
            } else {
                None
            }
        }
        PatKind::Struct(ref path, ref children, _) => {
            let m = resolve_ast_path(path, &scope.filepath, scope.point, session);
            let contextty = path_to_match(ty.clone(), session);
            if let Some(m) = m {
                let mut res = None;

                for child in children {
                    if point_is_in_span(point as u32, &child.span) {
                        res = typeinf::get_struct_field_type(&child.node.ident.name.as_str(), &m, session)
                            .and_then(|ty|
                                if let Some(Ty::Match(ref contextmatch)) = contextty {
                                    path_to_match_including_generics(ty, contextmatch, session)
                                } else {
                                    path_to_match(ty, session)
                                })
                            .and_then(|ty| destructure_pattern_to_ty(&child.node.pat, point, &ty, scope, session));

                        break;
                    }
                }

                res
            } else {
                None
            }
        }
        _ => {
            debug!("Could not destructure pattern {:?}", pat);
            None
        }
    }
}

struct LetTypeVisitor<'c: 's, 's> {
    scope: Scope,
    session: &'s Session<'c>,
    srctxt: String,
    pos: Point,        // pos is relative to the srctxt, scope is global
    result: Option<Ty>
}

impl<'c, 's> visit::Visitor for LetTypeVisitor<'c, 's> {
    fn visit_expr(&mut self, ex: &ast::Expr) {
        match ex.node {
            ExprKind::IfLet(ref pattern, ref expr, _, _) |
            ExprKind::WhileLet(ref pattern, ref expr, _, _) => {
                let mut v = ExprTypeVisitor{ scope: self.scope.clone(), result: None,
                                             session: self.session };
                v.visit_expr(expr);
                self.result = v.result.and_then(|ty|
                       destructure_pattern_to_ty(pattern, self.pos, &ty, &self.scope, self.session))
                    .and_then(|ty| path_to_match(ty, self.session));
            }
            _ => {
                visit::walk_expr(self, ex)
            }
        }
    }

    fn visit_local(&mut self, local: &ast::Local) {
        let mut ty = None;
        if let Some(ref local_ty) = local.ty {
            ty = to_racer_ty(local_ty, &self.scope);
        }

        if ty.is_none() {
            // oh, no type in the let expr. Try evalling the RHS
            ty = local.init.as_ref().and_then(|initexpr| {
                debug!("init node is {:?}", initexpr.node);
                let mut v = ExprTypeVisitor{ scope: self.scope.clone(), result: None,
                                             session: self.session };
                v.visit_expr(initexpr);
                v.result
            });
        }

        debug!("LetTypeVisitor: ty is {:?}. pos is {}, src is |{}|", ty, self.pos, self.srctxt);
        self.result = ty.and_then(|ty|
           destructure_pattern_to_ty(&local.pat, self.pos, &ty, &self.scope, self.session))
            .and_then(|ty| path_to_match(ty, self.session));
    }
}

struct MatchTypeVisitor<'c: 's, 's> {
    scope: Scope,
    session: &'s Session<'c>,
    pos: Point,        // pos is relative to the srctxt, scope is global
    result: Option<Ty>
}

impl<'c, 's> visit::Visitor for MatchTypeVisitor<'c, 's> {
    fn visit_expr(&mut self, ex: &ast::Expr) {
        if let ExprKind::Match(ref subexpression, ref arms) = ex.node {
            debug!("PHIL sub expr is {:?}", subexpression);

            let mut v = ExprTypeVisitor{ scope: self.scope.clone(), result: None,
                                         session: self.session };
            v.visit_expr(subexpression);

            debug!("PHIL sub type is {:?}", v.result);

            for arm in arms {
                for pattern in &arm.pats {
                    if point_is_in_span(self.pos as u32, &pattern.span) {
                        debug!("PHIL point is in pattern |{:?}|", pattern);
                        self.result = v.result.as_ref().and_then(|ty|
                               destructure_pattern_to_ty(pattern, self.pos, ty, &self.scope, self.session))
                            .and_then(|ty| path_to_match(ty, self.session));
                    }
                }
            }
        }
    }
}

fn resolve_ast_path(path: &ast::Path, filepath: &Path, pos: Point, session: &Session) -> Option<Match> {
    debug!("resolve_ast_path {:?}", to_racer_path(path));
    nameres::resolve_path_with_str(&to_racer_path(path), filepath, pos, core::SearchType::ExactMatch,
                                   core::Namespace::Both, session).nth(0)
}

fn to_racer_path(pth: &ast::Path) -> core::Path {
    let mut v = Vec::new();
    for seg in &pth.segments {
        let name = seg.identifier.name.to_string();
        let mut types = Vec::new();
        for ty in seg.parameters.types() {
            if let TyKind::Path(_, ref path) = ty.node {
                types.push(to_racer_path(path));
            }
        }
        v.push(core::PathSegment{ name: name, types: types });
    }
    core::Path{ global: pth.global, segments: v }
}

fn path_to_match(ty: Ty, session: &Session) -> Option<Ty> {
    match ty {
        Ty::PathSearch(ref path, ref scope) =>
            find_type_match(path, &scope.filepath, scope.point, session),
        Ty::RefPtr(ty) => {
            path_to_match(*ty, session)
        },
        _ => Some(ty)
    }
}

fn find_type_match(path: &core::Path, fpath: &Path, pos: Point, session: &Session) -> Option<Ty> {
    debug!("find_type_match {:?}, {:?}", path, fpath);
    let res = resolve_path_with_str(path, fpath, pos, core::SearchType::ExactMatch,
               core::Namespace::Type, session).nth(0).and_then(|m| {
                   match m.mtype {
                       MatchType::Type => get_type_of_typedef(m, session, fpath),
                       _ => Some(m)
                   }
               });

    res.and_then(|mut m| {
        // add generic types to match (if any)
        let types: Vec<core::PathSearch> = path.generic_types()
            .map(|typepath|
                 core::PathSearch{
                     path: typepath.clone(),
                     filepath: fpath.to_path_buf(),
                     point: pos
                 }).collect();

        if types.is_empty() {
            Some(Ty::Match(m))
        } else {
            m.generic_types = types;
            Some(Ty::Match(m))
        }
    })
}

fn get_type_of_typedef(m: Match, session: &Session, fpath: &Path) -> Option<Match> {
    debug!("get_type_of_typedef match is {:?}", m);
    let msrc = session.load_file_and_mask_comments(&m.filepath);
    let blobstart = m.point - 5;  // - 5 because 'type '
    let blob = msrc.from(blobstart);

    blob.iter_stmts().nth(0).and_then(|(start, end)| {
        let blob = msrc[blobstart + start..blobstart+end].to_owned();
        debug!("get_type_of_typedef blob string {}", blob);
        let res = parse_type(blob);
        debug!("get_type_of_typedef parsed type {:?}", res.type_);
        res.type_
    }).and_then(|type_| {
        let src = session.load_file(fpath);
        let scope_start = scopes::scope_start(src.as_src(), m.point);

        // Type of TypeDef cannot be inside the impl block so look outside
        let outer_scope_start = scope_start.checked_sub(1)
            .map(|sub| scopes::scope_start(src.as_src(), sub))
            .and_then(|s| {
                let blob = src.from(s);
                let blob = blob.trim_left();
                if blob.starts_with("impl") || blob.starts_with("trait") || blob.starts_with("pub trait") {
                    Some(s)
                } else {
                    None
                }
            });

        nameres::resolve_path_with_str(&type_,
                                       &m.filepath,
                                       outer_scope_start.unwrap_or(scope_start),
                                       core::SearchType::ExactMatch,
                                       core::Namespace::Type,
                                       session).nth(0)
    })
}

struct ExprTypeVisitor<'c: 's, 's> {
    scope: Scope,
    session: &'s Session<'c>,
    result: Option<Ty>,
}

impl<'c, 's> visit::Visitor for ExprTypeVisitor<'c, 's> {
    fn visit_expr(&mut self, expr: &ast::Expr) {
        debug!("visit_expr {:?}", expr);
        //walk_expr(self, ex, e)
        match expr.node {
            ExprKind::Unary(_, ref expr) |
            ExprKind::AddrOf(_, ref expr) => {
                self.visit_expr(expr);
            }
            ExprKind::Path(_, ref path) => {
                debug!("expr is a path {:?}", to_racer_path(path));
                let codemap::BytePos(lo) = path.span.lo;
                self.result = resolve_ast_path(path,
                                 &self.scope.filepath,
                                 self.scope.point + lo as usize,
                                 self.session).and_then(|m| {
                                     let msrc = self.session.load_file_and_mask_comments(&m.filepath);
                                     typeinf::get_type_of_match(m, msrc.as_src(), self.session)
                                 });
            }
            ExprKind::Call(ref callee_expression, _/*ref arguments*/) => {
                self.visit_expr(callee_expression);

                self.result = self.result.take().and_then(|m|
                    if let Ty::Match(m) = m {
                        match m.mtype {
                            MatchType::Function => typeinf::get_return_type_of_function(&m, &m, self.session)
                                .and_then(|ty| path_to_match(ty, self.session)),
                            MatchType::Struct => Some(Ty::Match(m)),
                            _ => {
                                debug!("ExprTypeVisitor: Cannot handle ExprCall of {:?} type", m.mtype);
                                None
                            }
                        }
                    } else {
                        None
                    }
                );
            }
            ExprKind::Struct(ref path, _, _) => {
                let pathvec = to_racer_path(path);
                self.result = find_type_match(&pathvec,
                                              &self.scope.filepath,
                                              self.scope.point,
                                              self.session);
            }

            ExprKind::MethodCall(ref spannedident, ref types, ref arguments) => {
                // spannedident.node is an ident I think
                let methodname = spannedident.node.name.to_string();
                debug!("method call ast name {}", methodname);
                debug!("method call ast types {:?} {}", types, types.len());

                let objexpr = &arguments[0];
                self.visit_expr(objexpr);

                self.result = self.result.as_ref().and_then(|contextm| {
                    match *contextm {
                        Ty::Match(ref contextm) => {
                            let omethod = nameres::search_for_impl_methods(
                                contextm,
                                &methodname,
                                contextm.point,
                                &contextm.filepath,
                                contextm.local,
                                core::SearchType::ExactMatch,
                                self.session);
                            omethod
                                .map(|method| typeinf::get_return_type_of_function(&method, contextm, self.session))
                                .filter_map(|ty| ty
                                     .and_then(|ty| {path_to_match_including_generics(ty, contextm, self.session)}))
                                .nth(0)
                        }
                        _ => None
                    }
                });
            }

            ExprKind::Field(ref subexpression, spannedident) => {
                let fieldname = spannedident.node.name.to_string();
                debug!("exprfield {}", fieldname);
                self.visit_expr(subexpression);
                self.result = self.result.as_ref()
                      .and_then(|structm|
                                match *structm {
                                    Ty::Match(ref structm) => {
                                typeinf::get_struct_field_type(&fieldname, structm, self.session)
                                .and_then(|fieldtypepath|
                                          find_type_match_including_generics(&fieldtypepath,
                                                                             &structm.filepath,
                                                                             structm.point,
                                                                             structm,
                                                                             self.session))
                                    },
                                    _ => None
                                });
            }

            ExprKind::Tup(ref exprs) => {
                let mut v = Vec::new();
                for expr in exprs {
                    self.visit_expr(expr);
                    match self.result {
                        Some(ref t) => v.push(t.clone()),
                        None => {
                            self.result = None;
                            return;
                        }
                    };
                }
                self.result = Some(Ty::Tuple(v));
            }

            ExprKind::Lit(ref lit) => {
                let ty_path = match lit.node {
                    LitKind::Str(_, _) => {
                        Some(core::Path::from_vec(false, vec!["str"]))
                    },
                    // See https://github.com/phildawes/racer/issues/727 for 
                    // information on why other literals aren't supported.
                    _ => None,
                };

                self.result = if let Some(lit_path) = ty_path {
                    find_type_match(&lit_path, &self.scope.filepath,
                                              self.scope.point,
                                              self.session)
                } else {
                    Some(Ty::Unsupported)
                };
            }

            ExprKind::TupField(ref subexpression, ref spanned_index) => {
                let fieldnum = spanned_index.node;
                debug!("tupfield {:?}", fieldnum);
                self.visit_expr(subexpression);
                self.result = self.result.as_ref().and_then(|ty| {
                    match *ty {
                        Ty::Match(ref structm) => {
                            typeinf::get_tuplestruct_field_type(fieldnum, structm, &self.session)
                                .and_then(|fieldtypepath|
                                    find_type_match_including_generics(&fieldtypepath, 
                                                                       &structm.filepath, 
                                                                       structm.point, 
                                                                       structm, 
                                                                       self.session))
                        }
                        _ => None
                    }
                });
            }

            ExprKind::Try(ref expr) => {                
                debug!("try expr");
                self.visit_expr(&expr);
                self.result = if let Some(&Ty::Match(ref m)) = self.result.as_ref() {
                    // HACK: Try to break open the result and find it's "Ok" type.
                    // Once the 'Try' operator trait stabilizes, it'd be better to
                    // find the type through the trait.
                    if m.matchstr == "Result" && m.generic_types.len() == 2 {
                        let ok_var = &m.generic_types[0];
                        find_type_match(&ok_var.path, 
                                        &ok_var.filepath, 
                                        ok_var.point, 
                                        self.session)
                    } else if m.matchstr == "Result" && (m.generic_types.len() != m.generic_args.len()) {
                        debug!("Unable to desugar Try expression; either `T` or `E` was `()`.");
                        None
                    } else {
                        debug!("Unable to desugar Try expression; type was {} with arity {} of {}", 
                            m.matchstr, 
                            m.generic_types.len(),
                            m.generic_args.len());
                        None
                    }
                } else {
                    None
                };
            }

            ExprKind::Match(_, ref arms) => {
                debug!("match expr");

                for arm in arms {
                    self.visit_expr(&arm.body);

                    // All match arms need to return the same result, so if we found a result
                    // we can end the search.
                    if self.result.is_some() {
                        break;
                    }
                }
            }

            ExprKind::If(_, ref block, ref else_block) |
            ExprKind::IfLet(_, _, ref block, ref else_block) => {
                debug!("if/iflet expr");

                visit::walk_block(self, &block);

                // if the block does not resolve to a type, try the else block
                if self.result.is_none() && else_block.is_some() {
                    self.visit_expr(&else_block.as_ref().unwrap());
                }
            }

            ExprKind::Block(ref block) => {
                debug!("block expr");
                visit::walk_block(self, &block);
            }

            _ => {
                debug!("- Could not match expr node type: {:?}",expr.node);
            }
        };
    }

    fn visit_mac(&mut self, mac: &ast::Mac) {
        // Just do nothing if we see a macro, but also prevent the panic! in the default impl.
        debug!("ignoring visit_mac: {:?}", mac);
    }
}

// gets generics info from the context match
fn path_to_match_including_generics(ty: Ty, contextm: &Match, session: &Session) -> Option<Ty> {
    match ty {
        Ty::PathSearch(ref fieldtypepath, ref scope) => {
            debug!("path_to_match_including_generics: {:?}  {:?}", ty, contextm);
            if fieldtypepath.segments.len() == 1 {
                // could have generic args! - try and resolve them
                let typename = fieldtypepath.segments[0].name.clone();
                let it = contextm.generic_args.iter().cloned()
                    .zip(contextm.generic_types.iter().cloned());
                let mut typepath = fieldtypepath.clone();
                let mut gentypefound = false;

                for (name, typesearch) in it.clone() {
                    if name == typename {
                        // yes! a generic type match!
                        return find_type_match(&typesearch.path,
                                               &typesearch.filepath,
                                               typesearch.point,
                                               session);
                    }

                    for typ in &mut typepath.segments[0].types {
                        let gentypename = typ.segments[0].name.clone();
                        if name == gentypename {
                            // A generic type on ty matches one on contextm
                            *typ = typesearch.path.clone(); // Overwrite the type with the one from contextm
                            gentypefound = true;
                        }
                    }
                }

                if gentypefound {
                    let mut out = find_type_match(&typepath, &scope.filepath, scope.point, session);

                    // Fix the paths on the generic types in out
                    if let Some(Ty::Match(ref mut m)) = out {
                        for (_, typesearch) in it {
                            for gentypematch in m.generic_types.iter_mut()
                                .filter(|ty| ty.path.segments[0].name == typesearch.path.segments[0].name) {
                                    *gentypematch = typesearch.clone();
                                }
                        }
                    }
                    return out;
                }
            }

            find_type_match(fieldtypepath, &scope.filepath, scope.point, session)
        }
        _ => Some(ty)
    }
}

fn find_type_match_including_generics(fieldtype: &core::Ty,
                                      filepath: &Path,
                                      pos: Point,
                                      structm: &Match,
                                      session: &Session) -> Option<Ty>{
    assert_eq!(&structm.filepath, filepath);
    let fieldtypepath = match *fieldtype {
        Ty::PathSearch(ref path, _) => path,
        Ty::RefPtr(ref ty) => match *ty.as_ref() {
            Ty::PathSearch(ref path, _) => path,
            _ => {
                debug!("EXPECTING A PATH!! Cannot handle other types yet. {:?}", fieldtype);
                return None
            }
        },
        _ => {
            debug!("EXPECTING A PATH!! Cannot handle other types yet. {:?}", fieldtype);
            return None
        }
    };

    if fieldtypepath.segments.len() == 1 {
        // could be a generic arg! - try and resolve it
        let typename = &fieldtypepath.segments[0].name;
        let it = structm.generic_args.iter()
            .zip(structm.generic_types.iter());
        for (name, typesearch) in it {
            if name == typename {
                // yes! a generic type match!
                return find_type_match(&typesearch.path,
                                       &typesearch.filepath,
                                       typesearch.point,
                                       session);
            }
        }
    }

    find_type_match(fieldtypepath, filepath, pos, session)
}


struct StructVisitor {
    pub scope: Scope,
    pub fields: Vec<(String, Point, Option<core::Ty>)>
}

impl visit::Visitor for StructVisitor {
    fn visit_variant_data(&mut self,
                          struct_definition: &ast::VariantData,
                          _: ast::Ident,
                          _: &ast::Generics,
                          _: ast::NodeId,
                          _: codemap::Span) {
        for field in struct_definition.fields() {
            let codemap::BytePos(point) = field.span.lo;

            let ty = to_racer_ty(&field.ty, &self.scope);
            let name = match field.ident {
                Some(ref ident) => ident.to_string(),
                // name unnamed field by its ordinal, since self.0 works
                None => format!("{}", self.fields.len()),
            };

            self.fields.push((name, point as usize, ty));
        }
    }
}

pub struct TypeVisitor {
    pub name: Option<String>,
    pub type_: Option<core::Path>
}

impl visit::Visitor for TypeVisitor {
    fn visit_item(&mut self, item: &ast::Item) {
        if let ItemKind::Ty(ref ty, _) = item.node {
            self.name = Some(item.ident.name.to_string());

            let typepath = match ty.node {
                TyKind::Rptr(_, ref ty) => {
                    match ty.ty.node {
                        TyKind::Path(_, ref path) => {
                            let type_ = to_racer_path(path);
                            debug!("type type is {:?}", type_);
                            Some(type_)
                        }
                        _ => None
                    }
                }
                TyKind::Path(_, ref path) => {
                    let type_ = to_racer_path(path);
                    debug!("type type is {:?}", type_);
                    Some(type_)
                }
                _ => None
            };
            self.type_ = typepath;
            debug!("typevisitor type is {:?}", self.type_);
        }
    }
}

pub struct TraitVisitor {
    pub name: Option<String>
}

impl visit::Visitor for TraitVisitor {
    fn visit_item(&mut self, item: &ast::Item) {
        if let ItemKind::Trait(_, _, _, _) = item.node {
            self.name = Some(item.ident.name.to_string());
        }
    }
}

#[derive(Debug)]
pub struct ImplVisitor {
    pub name_path: Option<core::Path>,
    pub trait_path: Option<core::Path>,
}

impl visit::Visitor for ImplVisitor {
    fn visit_item(&mut self, item: &ast::Item) {
        if let ItemKind::Impl(_, _, _, ref otrait, ref typ, _) = item.node {
            match typ.node {
                TyKind::Path(_, ref path) => {
                    self.name_path = Some(to_racer_path(path));
                }
                TyKind::Rptr(_, ref ty) => {
                    // HACK for now, treat refs the same as unboxed types
                    // so that we can match '&str' to 'str'
                    if let TyKind::Path(_, ref path) = ty.ty.node {
                        self.name_path = Some(to_racer_path(path));
                    }
                }
                _ => {}
            }
            otrait.as_ref().map(|t| {
                self.trait_path = Some(to_racer_path(&t.path));
            });
        }
    }
}

pub struct ModVisitor {
    pub name: Option<String>
}

impl visit::Visitor for ModVisitor {
    fn visit_item(&mut self, item: &ast::Item) {
        if let ItemKind::Mod(_) = item.node {
            self.name = Some(item.ident.name.to_string());
        }
    }
}

pub struct ExternCrateVisitor {
    pub name: Option<String>,
    pub realname: Option<String>
}

impl visit::Visitor for ExternCrateVisitor {
    fn visit_item(&mut self, item: &ast::Item) {
        if let ItemKind::ExternCrate(ref optional_s) = item.node {
            self.name = Some(item.ident.name.to_string());
            if let Some(ref istr) = *optional_s {
                self.realname = Some(istr.to_string());
            }
        }
    }
}

/// Wrapper struct for representing trait bounds.
/// Its usages are
/// - for generic types like T: Debug + Clone
/// - for trait inheritance like trait A: Debug + Clone
/// - for impl_trait like fn f(a: impl Debug + Clone)
/// - for dynamic traits(dyn_trait) like Box<Debug + Clone> or Box<dyn Debug + Clone>
#[derive(Clone, Debug, PartialEq)]
pub struct TraitBounds(Vec<core::PathSearch>);

impl TraitBounds {
    /// checks if it contains a trait, whick its name is 'name'
    pub fn find_by_name(&self, name: &str) -> Option<&core::PathSearch> {
        Some(self.0.iter().find(|path_search| {
            let seg = &path_search.path.segments;
            if seg.len() != 1 {
                return false;
            }
            &seg[0].name == name
        })?)
    }
    /// Search traits included in bounds and return Matches
    pub fn get_traits(&self, session: &Session) -> Vec<Match> {
        self.0.iter().filter_map(|ps| {
            resolve_path_with_str(&ps.path, &ps.filepath, ps.point, core::SearchType::ExactMatch,
                                  core::Namespace::Type, session).nth(0)
        }).collect()
    }
    fn from_ty_param_bounds<P: AsRef<Path>>(
        bounds: &TyParamBounds,
        file_path: P,
        offset: i32
    ) -> TraitBounds {
        let vec = bounds
            .iter()
            .filter_map(|bound| {
                if let TyParamBound::TraitTyParamBound(ref ptrait_ref, _) = *bound {
                    let ast_path = &ptrait_ref.trait_ref.path;
                    let codemap::BytePos(point) = ast_path.span.lo;
                    let path = to_racer_path(&ast_path);
                    let path_search = core::PathSearch {
                        path: path,
                        filepath: file_path.as_ref().to_path_buf(),
                        point: (point as i32 + offset) as Point,
                    };
                    Some(path_search)
                } else {
                    None
                }
            })
            .collect();
        TraitBounds(vec)
    }
}

/// Argument of generics like T: From<String>
#[derive(Clone, Debug)]
pub struct GenericsArg {
    /// Type parameter of generics, like 'T'
    pub name: String,
    /// The point 'T' appears
    pub point: usize,
    /// bounds
    pub bounds: TraitBounds,
}

impl GenericsArg {
    pub fn name(&self) -> &str {
        &(*self.name)
    }
    fn into_match<P: AsRef<Path>>(self, filepath: &P) -> Option<Match> {
        // TODO: contextstr, local
        Some(Match {
            matchstr: self.name,
            filepath: filepath.as_ref().to_path_buf(),
            point: self.point,
            coords: None,
            local: false,
            mtype: MatchType::TraitBounds(self.bounds),
            contextstr: String::new(),
            generic_args: Vec::new(),
            generic_types: Vec::new(),
            docs: String::new(),
        })
    }
}

/// List of Some GenericsArgs like <T: Clone, U, P>
#[derive(Clone, Debug, Default)]
pub struct GenericsList {
    pub inner: Vec<GenericsArg>
}

impl GenericsList {
    fn find_type_param(&self, name: &str) -> Option<&GenericsArg> {
        self.inner.iter().find(|v| &v.name == name)
    }
    fn from_generics<'a, P: AsRef<Path>>(
        generics: &'a Generics,
        file_path: P,
        offset: i32
    ) -> GenericsList {
        let ty_params = generics.ty_params.iter();
        let res = ty_params
            .map(|ty_param| {
                let param_name = ty_param.ident.name.as_str().to_string();
                let codemap::BytePos(point) = ty_param.span.lo;
                let bounds = TraitBounds::from_ty_param_bounds(&ty_param.bounds, &file_path, offset);
                GenericsArg {
                    name: param_name,
                    point: (point as i32 + offset) as Point,
                    bounds: bounds,
                }
            })
            .collect();
        GenericsList { inner: res }
    }
    pub fn get_idents(&self) -> Vec<String> {
        self.inner.iter().map(|g| g.name.to_string()).collect()
    }
}

#[derive(Debug)]
struct GenericsVisitor<P> {
    result: GenericsList,
    filepath: P,
}

impl<P: AsRef<Path>> visit::Visitor for GenericsVisitor<P> {
    fn visit_generics(&mut self, g: &ast::Generics) {
        let path = &self.filepath;
        self.result.inner.extend(GenericsList::from_generics(g, path, 0).inner);
    }
}


pub struct EnumVisitor {
    pub name: String,
    pub values: Vec<(String, Point)>
}

impl visit::Visitor for EnumVisitor {
    fn visit_item(&mut self, i: &ast::Item) {
        if let ItemKind::Enum(ref enum_definition, _) = i.node {
            self.name = i.ident.name.to_string();
            //visitor.visit_generics(type_parameters, env.clone());
            //visit::walk_enum_def(self, enum_definition, type_parameters, e)

            let codemap::BytePos(point) = i.span.lo;
            let codemap::BytePos(point2) = i.span.hi;
            debug!("name point is {} {}", point, point2);

            for variant in &enum_definition.variants {
                let codemap::BytePos(point) = variant.span.lo;
                self.values.push((variant.node.name.to_string(), point as usize));
            }
        }
    }
}

pub fn parse_use(s: String) -> UseVisitor {
    let mut v = UseVisitor{ ident: None, paths: Vec::new(), is_glob: false };
    if let Some(cr) = string_to_crate(s) {
        visit::walk_crate(&mut v, &cr);
    }
    v
}

pub fn parse_pat_bind_stmt(s: String) -> Vec<SourceByteRange> {
    let mut v = PatBindVisitor{ ident_points: Vec::new() };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v.ident_points
}

pub fn parse_struct_fields(s: String, scope: Scope) -> Vec<(String, Point, Option<core::Ty>)> {
    let mut v = StructVisitor{ scope: scope, fields: Vec::new() };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v.fields
}

pub fn parse_impl(s: String) -> ImplVisitor {
    let mut v = ImplVisitor { name_path: None, trait_path: None };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v
}

pub fn parse_trait(s: String) -> TraitVisitor {
    let mut v = TraitVisitor { name: None };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v
}

/// parse traits and collect inherited traits as TraitBounds
pub fn parse_inherited_traits<P: AsRef<Path>>(
    s: String,
    filepath: P,
    offset: i32
) -> Option<TraitBounds>{
    let mut v = InheritedTraitsVisitor {
        result: None,
        file_path: filepath,
        offset: offset,
    };
    let stmt = string_to_stmt(s)?;
    visit::walk_stmt(&mut v, &stmt);
    v.result
}

pub fn parse_generics<P: AsRef<Path>>(s: String, filepath: P) -> GenericsList {
    let mut v = GenericsVisitor {
        result: GenericsList::default(),
        filepath: filepath,
    };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v.result
}

pub fn parse_type(s: String) -> TypeVisitor {
    let mut v = TypeVisitor { name: None, type_: None };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v
}

pub fn parse_fn_args(s: String) -> Vec<SourceByteRange> {
    parse_pat_idents(s)
}

pub fn parse_pat_idents(s: String) -> Vec<SourceByteRange> {
    let mut v = PatVisitor{ ident_points: Vec::new() };
    if let Some(stmt) = string_to_stmt(s) {
        debug!("parse_pat_idents stmt is {:?}", stmt);
        visit::walk_stmt(&mut v, &stmt);
        debug!("ident points are {:?}", v.ident_points);
    }
    v.ident_points
}


pub fn parse_fn_output(s: String, scope: Scope) -> Option<core::Ty> {
    let mut v = FnOutputVisitor { result: None, scope: scope };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v.result
}

pub fn parse_fn_arg_type(
    s: String,
    argpos: Point,
    scope: Scope,
    session: &Session,
    offset: i32,
) -> Option<core::Ty> {
    debug!("parse_fn_arg {} |{}|", argpos, s);
    let mut v = FnArgTypeVisitor {
        argpos: argpos,
        scope: scope,
        result: None,
        offset: offset,
        session: session,
    };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v.result
}


pub fn parse_mod(s: String) -> ModVisitor {
    let mut v = ModVisitor { name: None };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v
}

pub fn parse_extern_crate(s: String) -> ExternCrateVisitor {
    let mut v = ExternCrateVisitor { name: None, realname: None };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v
}

pub fn parse_enum(s: String) -> EnumVisitor {
    let mut v = EnumVisitor { name: String::new(), values: Vec::new() };
    if let Some(stmt) = string_to_stmt(s) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v
}

pub fn get_type_of(exprstr: String, fpath: &Path, pos: Point, session: &Session) -> Option<Ty> {
    let startscope = Scope {
        filepath: fpath.to_path_buf(),
        point: pos
    };

    let mut v = ExprTypeVisitor{ scope: startscope, result: None, session: session };

    if let Some(stmt) = string_to_stmt(exprstr) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v.result
}

// pos points to an ident in the lhs of the stmtstr
pub fn get_let_type(stmtstr: String, pos: Point, scope: Scope, session: &Session) -> Option<Ty> {
    let mut v = LetTypeVisitor {
        scope: scope,
        session: session,
        srctxt: stmtstr.clone(),
        pos: pos,
        result: None
    };
    if let Some(stmt) = string_to_stmt(stmtstr) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v.result
}

pub fn get_match_arm_type(stmtstr: String, pos: Point, scope: Scope, session: &Session) -> Option<Ty> {
    let mut v = MatchTypeVisitor {
        scope: scope,
        session: session,
        pos: pos,
        result: None
    };
    if let Some(stmt) = string_to_stmt(stmtstr) {
        visit::walk_stmt(&mut v, &stmt);
    }
    v.result
}

pub struct FnOutputVisitor {
    scope: Scope,
    pub result: Option<Ty>
}

impl visit::Visitor for FnOutputVisitor {
    fn visit_fn(&mut self,  _: visit::FnKind, fd: &ast::FnDecl, _: codemap::Span, _: ast::NodeId) {
        self.result = match fd.output {
            FunctionRetTy::Ty(ref ty) => to_racer_ty(ty, &self.scope),
            FunctionRetTy::Default(_) => None
        };
    }
}

/// Visitor to detect type of fnarg
pub struct FnArgTypeVisitor<'c: 's, 's> {
    /// the code point arg appears in search string
    argpos: Point,
    scope: Scope,
    session: &'s Session<'c>,
    /// the code point search string starts
    /// use i32 for the case `impl blah {` in inserted
    offset: i32,
    pub result: Option<Ty>
}


impl<'c, 's> visit::Visitor for FnArgTypeVisitor<'c, 's> {
    fn visit_fn(&mut self, fk: visit::FnKind, fd: &ast::FnDecl, _: codemap::Span, _: ast::NodeId) {
        // Get generics arguments here (just for speed up)
        let filepath = &self.scope.filepath;
        let generics_list = match fk {
            visit::FnKind::ItemFn(_, generics, _, _, _, _, _) => {
                GenericsList::from_generics(generics, filepath, self.offset)
            }
            visit::FnKind::Method(_, method_sig, _, _) => {
                GenericsList::from_generics(&method_sig.generics, filepath, self.offset)
            }
            _ => GenericsList::default(),
        };
        for arg in &fd.inputs {
            if point_is_in_span(self.argpos as u32, &arg.pat.span) {
                debug!("fn arg visitor found type {:?}", arg.ty);
                self.result = to_racer_ty(&arg.ty, &self.scope)
                    .and_then(|ty| destructure_pattern_to_ty(&arg.pat, self.argpos,
                                                             &ty, &self.scope, self.session))
                    .and_then(|ty| {
                        let ty = destruct_ty_refptr(ty);
                        if let Ty::PathSearch(ref path, ref scope) = ty {
                            let segments = &path.segments;
                            if segments.len() == 1 {
                                let name = &segments[0].name;
                                if let Some(bounds) = generics_list.find_type_param(name) {
                                    let res = bounds.to_owned().into_match(filepath)?;
                                    return Some(Ty::Match(res));
                                }
                            }
                            find_type_match(path, &scope.filepath, scope.point, self.session)
                        } else {
                            Some(ty)
                        }
                    });
                return;
            }
        }
    }
}

fn destruct_ty_refptr(ty_arg: Ty) -> Ty {
    if let Ty::RefPtr(ty) = ty_arg {
        destruct_ty_refptr(*ty)
    } else {
        ty_arg
    }
}

/// Visitor to collect Inherited Traits
pub struct InheritedTraitsVisitor<P> {
    /// search result(list of Inherited Traits)
    result: Option<TraitBounds>,
    /// the file trait appears
    file_path: P,
    /// thecode point 'trait' statement starts
    offset: i32,
}

impl<P: AsRef<Path>> visit::Visitor for InheritedTraitsVisitor<P> {
    fn visit_item(&mut self, item: &ast::Item) {
        if let ItemKind::Trait(_, _, ref bounds, _) = item.node {
            self.result = Some(TraitBounds::from_ty_param_bounds(
                bounds,
                &self.file_path,
                self.offset,
            ));
        }
    }
}
