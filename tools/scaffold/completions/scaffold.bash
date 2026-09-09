# Completion for the scaffold shim. Source it, or drop it in
# /usr/share/bash-completion/completions/ under the name "scaffold".

_scaffold_root() {
    local dir="$PWD"
    while [ -n "$dir" ]; do
        if [ -x "$dir/scaffold" ] && [ -f "$dir/Cargo.toml" ]; then
            printf '%s\n' "$dir"
            return 0
        fi
        dir="${dir%/*}"
    done
    return 1
}

_scaffold_topics() {
    local root
    root="$(_scaffold_root)" || return 0
    if [ -x "$root/tools/scaffold/target/debug/scaffold" ]; then
        "$root/tools/scaffold/target/debug/scaffold" --topics 2>/dev/null
    else
        "$root/scaffold" --topics 2>/dev/null
    fi
}

_scaffold() {
    local current previous index positionals skip word
    current="${COMP_WORDS[COMP_CWORD]}"
    previous="${COMP_WORDS[COMP_CWORD-1]}"

    case "$previous" in
        -d|--dir)
            COMPREPLY=()
            return
            ;;
    esac

    if [[ $current == -* ]]; then
        COMPREPLY=($(compgen -W '-d --dir -l --lib -n --dry-run -t --topics -h --help' -- "$current"))
        return
    fi

    index=1
    positionals=0
    skip=0
    while [ "$index" -lt "$COMP_CWORD" ]; do
        word="${COMP_WORDS[index]}"
        if [ "$skip" -eq 1 ]; then
            skip=0
        elif [ "$word" = "-d" ] || [ "$word" = "--dir" ]; then
            skip=1
        elif [[ $word != -* ]]; then
            positionals=$((positionals + 1))
        fi
        index=$((index + 1))
    done

    if [ "$positionals" -eq 0 ]; then
        COMPREPLY=($(compgen -W "$(_scaffold_topics)" -- "$current"))
    else
        COMPREPLY=()
    fi
}

complete -F _scaffold scaffold ./scaffold
