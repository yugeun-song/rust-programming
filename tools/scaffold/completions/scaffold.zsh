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

_scaffold_topics() {
    local root
    root="$(_scaffold_root)" || return 0
    if [[ -x $root/tools/scaffold/target/debug/scaffold ]]; then
        "$root/tools/scaffold/target/debug/scaffold" --topics 2>/dev/null && return 0
    fi
    "$root/scaffold" --topics 2>/dev/null
}

_scaffold() {
    local -a topics
    topics=(${(f)"$(_scaffold_topics)"})

    _arguments \
        '*'{-d,--dir}'[a program spanning several files]:name:' \
        '(-l --lib)'{-l,--lib}'[add src/lib.rs, shared by the topic programs]' \
        '(-n --dry-run)'{-n,--dry-run}'[print the plan and write nothing]' \
        '(-t --topics)'{-t,--topics}'[print the known topic names]' \
        '(-h --help)'{-h,--help}'[show the usage text]' \
        "1:topic:(${topics})" \
        '*:program name:'
}

if [[ $funcstack[1] == _scaffold ]]; then
    _scaffold "$@"
else
    compdef _scaffold scaffold ./scaffold
fi
