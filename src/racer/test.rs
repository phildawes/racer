use racer::complete_from_file;
use racer::find_definition;
use std::io::File;
use std::task;
use racer::scopes;

fn tmpname() -> Path {
    let s = task::name().unwrap();
    let mut p = String::from_str("tmpfile.");
    p.push_str(s.as_slice());
    Path::new(p)
}

fn write_file(tmppath:&Path, s : &str) {
    let mut f = File::create(tmppath);
    f.write(s.as_bytes()).unwrap();
    f.flush().unwrap();
}

fn remove_file(tmppath:&Path) {
    ::std::io::fs::unlink(tmppath).unwrap();
}

#[test]
fn completes_fn() {
    let src="
    fn apple() {
    }

    fn main() {
        let b = ap
    }";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 6, 18);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("apple".to_string(), got.matchstr.to_string());
}

#[test]
fn completes_pub_fn_locally() {
    let src="
    pub fn apple() {
    }

    fn main() {
        let b = ap
    }";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 6, 18);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("apple".to_string(), got.matchstr.to_string());
}

#[test]
fn completes_local_scope_let(){
    let src="
    fn main() {
        let apple = 35;
        let b = ap
    }";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 4, 18);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("apple".to_string(), got.matchstr);
    assert_eq!(29, got.point);
}

#[test]
fn completes_via_parent_scope_let(){
    let src="
fn main() {
    let mut apple = 35;
    if foo {
        let b = ap
    }
}";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 5, 18);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("apple".to_string(), got.matchstr);
    assert_eq!(25, got.point);
}

#[test]
fn follows_use() {
    let src2="
    pub fn myfn() {}
    pub fn foo() {}
    ";
    let src="
    use src2::{foo,myfn};
    mod src2;
    fn main() {
        myfn();
    }
    ";
    write_file(&Path::new("src2.rs"), src2);
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 5, 10);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"myfn".to_string());
}

#[test]
fn completes_struct_field_via_assignment() {
    let src="
    struct Point {
        first: f64,
        second: f64
    } 

    let var = Point {first: 35, second: 22};
    var.f
";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 8, 9);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("first".to_string(), got.matchstr);
}

#[test]
fn finds_defn_of_struct_field() {
    let src="
    struct Point {
        first: f64,
        second: f64
    } 

    let var = Point {first: 35, second: 22};
    var.first
";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 8, 9);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"first".to_string());
}

#[test]
fn finds_impl_fn() {
    let src="
    struct Foo;
    impl Foo {
        fn new() {}
    }

    Foo::new();
";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 7, 10);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"new".to_string());
}

#[test]
fn follows_use_to_inline_mod() {
    let src="
    use foo::myfn;
    mod foo {
        pub fn myfn() {}
    }

    fn main() {
        myfn();
    }
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 8, 9);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"myfn".to_string());
}

#[test]
fn finds_enum() {
    let src="
    enum MyEnum {
        One, Two
    }
    
    fn myfn(e: MyEnum) {}
    ";
    write_file(&Path::new("src.rs"), src);
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 6, 16);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"MyEnum".to_string());
}

#[test]
fn finds_type() {
    let src="
    type SpannedIdent = Spanned<Ident>
    SpannedIdent;
    ";
    write_file(&Path::new("src.rs"), src);
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 3, 5);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"SpannedIdent".to_string());
}

#[test]
fn finds_trait() {
    let src="
    pub trait MyTrait<E: Clone> {}
    MyTrait
    ";
    write_file(&Path::new("src.rs"), src);
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 3, 5);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"MyTrait".to_string());
}


#[test]
fn finds_fn_arg() {
    let src="
    fn myfn(myarg: &str) {
         myarg
    }
    ";
    write_file(&Path::new("src.rs"), src);
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 3, 10);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"myarg".to_string());
}


#[test]
fn finds_enum_value() {
    let src="
    enum MyEnum {
        One, Two
    }

    Two;
    ";
    write_file(&Path::new("src.rs"), src);
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 6, 6);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"Two".to_string());
}

#[test]
fn finds_inline_fn() {
    let src="
    #[inline]
    fn contains<'a>(&needle: &'a str) -> bool {
    }

    contains();
    ";
    write_file(&Path::new("src.rs"), src);
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 6, 9);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"contains".to_string());
}

#[test]
fn follows_self_use() {
    let modsrc = "
    pub use self::src2::{Foo,myfn};
    pub mod src2;
    ";
    let src2 = "
    struct Foo;
    pub fn myfn() {}
    ";
    let src = "
    use mymod::{Foo,myfn};
    pub mod mymod;

    fn main() {
        myfn();
    }
    ";
    let basedir = tmpname();
    let moddir = basedir.join("mymod");
    ::std::io::fs::mkdir_recursive(&moddir, ::std::io::UserRWX).unwrap();

    write_file(&moddir.join("mod.rs"), modsrc);
    write_file(&moddir.join("src2.rs"), src2);
    let srcpath = basedir.join("src.rs");
    write_file(&srcpath, src);
    let pos = scopes::coords_to_point(src, 6, 10);
    let got = find_definition(src, &srcpath, pos).unwrap();
    ::std::io::fs::rmdir_recursive(&basedir).unwrap();
    assert_eq!(got.matchstr,"myfn".to_string());
    assert_eq!(moddir.join("src2.rs").display().to_string(), 
               got.filepath.display().to_string());
    assert_eq!(28, got.point);
}

#[test]
fn finds_nested_submodule_file() {
    let rootsrc = "
    pub mod sub1 {
        pub mod sub2 {
            pub mod sub3;
        }
    }
    sub1::sub2::sub3::myfn();
    ";

    let sub3src = "
    pub fn myfn() {}
    ";

    let basedir = tmpname();
    let srcpath = basedir.join("root.rs");
    let sub2dir = basedir.join("sub1").join("sub2");
    ::std::io::fs::mkdir_recursive(&sub2dir, ::std::io::UserRWX).unwrap();
    write_file(&srcpath, rootsrc);
    write_file(&sub2dir.join("sub3.rs"), sub3src);
    let pos = scopes::coords_to_point(rootsrc, 7, 23);
    let got = find_definition(rootsrc, &srcpath, pos).unwrap();
    ::std::io::fs::rmdir_recursive(&basedir).unwrap();
    assert_eq!(got.matchstr,"myfn".to_string());
    assert_eq!(sub2dir.join("sub3.rs").display().to_string(), 
               got.filepath.display().to_string());
}


#[test]
fn follows_use_to_impl() {
    let modsrc = "
    pub struct Foo;
    impl Foo {       // impl doesn't need to be 'pub'
        pub fn new() -> Foo {
            Foo
        }
    }
    ";
    let src = "
    use mymod::{Foo};
    mod mymod;
    fn main() {
        Foo::new();
    }
    ";
    let basedir = tmpname();
    ::std::io::fs::mkdir_recursive(&basedir, ::std::io::UserRWX).unwrap();

    let modpath = basedir.join("mymod.rs");
    write_file(&modpath, modsrc);
    let srcpath = basedir.join("src.rs");
    write_file(&srcpath, src);
    let pos = scopes::coords_to_point(src, 5, 14);
    let got = find_definition(src, &srcpath, pos).unwrap();

    ::std::io::fs::rmdir_recursive(&basedir).unwrap();
    assert_eq!(got.matchstr,"new".to_string());
    assert_eq!(90, got.point);
    assert_eq!(modpath.display().to_string(), 
               got.filepath.display().to_string());
}

#[test]
fn finds_templated_impl_fn() {
    let src="
    struct Foo<T>;
    impl<T> Foo<T> {
        fn new() {}
    }

    Foo::new();
";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 7, 10);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!(got.matchstr,"new".to_string());
}

#[test]
fn follows_fn_to_method() {
    let src="
    struct Foo<T>;
    impl<T> Foo<T> {
        fn new() -> Foo<T> {}
        fn mymethod(&self) {}
    }

    fn main() {
        let v = Foo::new();
        v.my
    }
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 10, 12);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("mymethod".to_string(), got.matchstr);
}

#[test]
fn follows_arg_to_method() {
    let src="
    struct Foo<T>;
    impl<T> Foo<T> {
        fn mymethod(&self) {}
    }

    fn myfn(v: &Foo) {
        v.my
    }
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 8, 12);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("mymethod".to_string(), got.matchstr);
}

#[test]
fn follows_arg_to_enum_method() {
    let src="
    enum Foo<T> {
       EnumVal
    }
    impl<T> Foo<T> {
        fn mymethod(&self) {}
    }

    fn myfn(v: &Foo) {
        v.my
    }
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 10, 12);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("mymethod".to_string(), got.matchstr);
}

#[test]
fn follows_let_method_call() {
    let src="
    struct Foo;
    struct Bar;
    impl<T> Foo<T> {
        fn mymethod(&self) -> Bar {}
    }
    impl<T> Bar<T> {
        fn mybarmethod(&self) -> Bar {}
    }

    fn myfn(v: &Foo) {
        let f = v.mymethod();
        f.my
    }
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 13, 12);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("mybarmethod".to_string(), got.matchstr);
}

#[test]
fn follows_chained_method_call() {
    let src="
    struct Foo;
    struct Bar;
    impl<T> Foo<T> {
        fn mymethod(&self) -> Bar {}
    }
    impl<T> Bar<T> {
        fn mybarmethod(&self) -> Bar {}
    }

    fn myfn(v: &Foo) {
        v.mymethod().my
    }
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 12, 23);
    let got = complete_from_file(src, &path, pos).nth(0).unwrap();
    remove_file(&path);
    assert_eq!("mybarmethod".to_string(), got.matchstr);
}

#[test]
fn differentiates_type_and_value_namespaces() {
    let src = "
    enum MyEnum{ Foo }
    struct Foo;
    impl Foo { pub fn new() -> Foo {} }
    let l = Foo::new();
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 5, 18);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    println!("PHIL {}",got.matchstr);
    println!("PHIL {}",got.mtype);
    assert_eq!("new", got.matchstr.as_slice());
}

#[test]
fn follows_self_to_method() {
    let src= "
    struct Foo;
    impl Bar for Foo {
        pub fn method(self) {
        }

        pub fn another_method(self) {
            self.method()
        }
    }";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 8, 20);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("method", got.matchstr.as_slice());    
}

#[test]
fn follows_self_to_trait_method() {
    let src= "
    trait Bar {
        pub fn method(self) {
        }
        pub fn another_method(self) {
            self.method()
        }
    }";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 6, 20);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("method", got.matchstr.as_slice());    
}

#[test]
fn finds_trait_method() {
    let src = "
    pub trait MyTrait {
        fn op(self);
        fn trait_method(self){} 
    } 

    struct Foo;
    impl MyTrait for Foo {
        fn op(self) {
            self.trait_method();
        }
    }";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 10, 22);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("trait_method", got.matchstr.as_slice());
}


#[test]
fn finds_field_type() {
    let src = "
    pub struct Blah { subfield: uint }

    pub struct Foo {
        myfield : Blah
    }

    let f = Foo{ myfield: Blah { subfield: 3}};
    f.myfield.subfield
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 9, 16);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("subfield", got.matchstr.as_slice());
}

#[test]
fn finds_a_generic_retval_from_a_function() {
    let src="
    pub struct Blah { subfield: uint }
    pub struct Foo<T> {
        myfield: T
    }
    fn myfn() -> Foo<Blah> {}
    myfn().myfield.subfield
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 7, 24);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("subfield", got.matchstr.as_slice());
}

#[test]
fn handles_an_enum_option_style_return_type() {
    let src="
    pub struct Blah { subfield: uint }
    pub enum MyOption<T> {
        MySome(T),
        MyNone
    }
    impl MyOption<T> {
         pub fn unwrap(&self) -> T {}
    }
    fn myfn() -> MyOption<Blah> {}
    let s = myfn();
    s.unwrap().subfield
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 12, 18);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("subfield", got.matchstr.as_slice());
}

#[test]
fn finds_definition_of_lambda_argument() {
    let src="
    fn myfn(&|int|) {}
    myfn(|a|a+3);
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 3, 12);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("a", got.matchstr.as_slice());
}

#[test]
fn finds_definition_of_let_tuple() {
    let src="
    let (a, b) = (2,3);
    a
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 3, 4);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("a", got.matchstr.as_slice());
}

#[test]
fn finds_type_of_tuple_member_via_let_type() {
    let src="
    pub struct Blah { subfield: uint }
    let (a, b): (uint, Blah);
    b.subfield
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 4, 11);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("subfield", got.matchstr.as_slice());
}


#[test]
fn finds_type_of_tuple_member_via_let_expr() {
    let src="
    pub struct Blah { subfield: uint }
    let (a, b) = (3, Blah{subfield:3});
    b.subfield
    ";
    let path = tmpname();
    write_file(&path, src);
    let pos = scopes::coords_to_point(src, 4, 11);
    let got = find_definition(src, &path, pos).unwrap();
    remove_file(&path);
    assert_eq!("subfield", got.matchstr.as_slice());
}


// #[test]
// fn finds_methods_of_string_slice() {
//     let src = "
//     fn strargfn(s: &str) {
//             s.
//     }
//     ";
//     let path = tmpname();
//     write_file(&path, src);
//     let pos = scopes::coords_to_point(src, 3, 10);
//     let mut it = complete_from_file(src, &path, pos);
//     remove_file(&path);
//     let mut found = false;
//     for m in it {
//         println!("got {}",m.matchstr);
//         if m.matchstr.as_slice() == "contains" {
//             found = true;
//             break;
//         }
//     }
//     assert!(found);
// }
