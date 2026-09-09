#compdef scaffold
# Completion for the scaffold shim. Source it, or put it on $fpath as
# "_scaffold" and let compinit autoload it.

_scaffold_root() {
    local dir="$PWD"
    while [[ -n $dir ]]; do
        if [[ -x $dir/scaffold && -f $dir/Cargo.toml ]]; then
            print -r -- "$dir"
            return 0
        fi
        dir="${dir%/*}"
    done
    return 1
}

_scaffold() {
    local root
    local -a topics
    if root="$(_scaffold_root)"; then
        if [[ -x $root/tools/scaffold/target/debug/scaffold ]]; then
            topics=(${(f)"$("$root/tools/scaffold/target/debug/scaffold" --topics 2>/dev/null)"})
        else
            topics=(${(f)"$("$root/scaffold" --topics 2>/dev/null)"})
        fi
    fi

    _arguments -s \
        '(-d --dir)'{-d,--dir}'[a program spanning several files]:name:' \
        '(-l --lib)'{-l,--lib}'[add src/lib.rs, shared by the topic programs]' \
        '(-n --dry-run)'{-n,--dry-run}'[print the plan and write nothing]' \
        '(-t --topics)'{-t,--topics}'[print the known topic names]' \
        '(-h --help)'{-h,--help}'[show the usage text]' \
        "1:topic:(${topics})" \
        '*:program name:'
}

compdef _scaffold scaffold ./scaffold
