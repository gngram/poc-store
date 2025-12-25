{ 
  pkgs ? import <nixpkgs>{},
}:

pkgs.mkShell {
  buildInputs = [ 
    pkgs.gcc
    pkgs.git
    pkgs.cargo
  ];  

  shellHook = ''
    export TERM="xterm-256color"
    parse_git_branch() {
      git branch 2> /dev/null | sed -e '/^[^*]/d' -e 's/* \(.*\)/ (\1)/'
    }

    CYAN='\[\033[0;36m\]'
    WHITE='\[\033[0;37m\]'
    BLUE='\[\033[0;34m\]'
    GREEN='\[\033[0;32m\]'       # Fixed: 42m is background color, use 32m for green text
    YELLOW='\[\033[0;33m\]'      # Fixed: 103m is background color, use 33m for yellow text
    RED='\[\033[0;31m\]'
    TEXT_RESET='\[\033[0m\]'     # Reset all attributes
    TIME='\t'
    CURRENT_PATH='\w'


    export PS1="$BLUE<DEV>$RED [$YELLOW $CURRENT_PATH $TEXT_RESET $RED]:$GREEN\$(parse_git_branch)$GREEN$ROOT_OR_NOT:$TEXT_RESET "
    echo "Welcome to your Rust development environment."
  '';
}

