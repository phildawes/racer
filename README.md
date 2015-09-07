# *Racer* - code completion for [Rust](http://www.rust-lang.org/)

[![Build Status](https://travis-ci.org/phildawes/racer.svg?branch=master)](https://travis-ci.org/phildawes/racer)

![racer completion screenshot](images/racer_completion.png)

![racer eldoc screenshot](images/racer_eldoc.png)

*RACER* = *R*ust *A*uto-*C*omplete-*er*. A utility intended to provide rust code completion for editors and IDEs. Maybe one day the 'er' bit will be exploring + refactoring or something.

## Installation

1. Clone the repository: ```git clone https://github.com/phildawes/racer.git```

2. ```cd racer; cargo build --release```.  The binary will now be in ```./target/release/racer```

3. Fetch the rust sourcecode from git, or download from https://www.rust-lang.org/install.html

4. Set the ```RUST_SRC_PATH``` env variable to point to the 'src' dir in the rust source installation

   (e.g. ```% export RUST_SRC_PATH=/usr/local/src/rust/src``` )

5. Test on the command line:

   ```./target/release/racer complete std::io::B ```  (should show some completions)


## Emacs integration

Emacs intergation has been moved to a separate project: [emacs-racer](https://github.com/racer-rust/emacs-racer)

## Vim integration

1. Install using Pathogen, Vundle or NeoBundle. Or, copy racer/plugin/racer.vim into your .vim/plugin directory.

  Vundle users:
  ```
  Plugin 'phildawes/racer'
  ```

  NeoBundle users:
  ```
  NeoBundle 'phildawes/racer', {
  \   'build' : {
  \     'mac': 'cargo build --release',
  \     'unix': 'cargo build --release',
  \   }
  \ }
  ```

2. Add g:racer_cmd and $RUST_SRC_PATH variables to your .vimrc. Also it's worth turning on 'hidden' mode for buffers otherwise you need to save the current buffer every time you do a goto-definition. E.g.:

     ```
     set hidden
     let g:racer_cmd = "<path-to-racer>/target/release/racer"
     let $RUST_SRC_PATH="<path-to-rust-srcdir>/src/"
     ```

3. In insert mode use C-x-C-o to search for completions

4. In normal mode type 'gd' to go to a definition

## Kate integration

The Kate community maintains a [plugin](http://quickgit.kde.org/?p=kate.git&a=tree&&f=addons%2Frustcompletion). It will be bundled with future releases of Kate (read more [here](https://blogs.kde.org/2015/05/22/updates-kates-rust-plugin-syntax-highlighting-and-rust-source-mime-type)).

1. Enable 'Rust code completion' in the plugin list in the Kate config dialog

2. On the new 'Rust code completion' dialog page, make sure 'Racer command' and 'Rust source tree location' are set correctly
