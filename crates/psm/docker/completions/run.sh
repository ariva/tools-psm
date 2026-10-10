#!/usr/bin/env bash
# Runs every case of cases.txt through bash, zsh and fish completion and
# fails on the first expectation that does not hold. Prints what each
# shell offered when it does.
set -u
shopt -s extglob
cd "$(dirname "$0")"
fail=0; total=0
started=$(date +%s%N)
declare -A spent=([bash]=0 [zsh]=0 [fish]=0)

# bash: call the completion function the way readline would.
complete_bash() {
    bash -c '
        source /etc/bash_completion.d/psm
        read -ra COMP_WORDS <<< "$1"
        [[ "$1" == *" " ]] && COMP_WORDS+=("")
        COMP_CWORD=$(( ${#COMP_WORDS[@]} - 1 ))
        COMP_LINE="$1"; COMP_POINT=${#1}
        _psm psm "${COMP_WORDS[COMP_CWORD]}" "${COMP_WORDS[COMP_CWORD-1]}"
        printf "%s\n" "${COMPREPLY[@]}"' _ "$1"
}

# fish: the built-in that prints what Tab would offer, one per line with a description.
complete_fish() {
    fish -c "complete -C '$1'" | cut -f1
}

# zsh: needs a terminal; zsh-complete.zsh drives an interactive zsh in a zpty.
complete_zsh() {
    zsh ./zsh-complete.zsh "$1"
}

while IFS='|' read -r shells line expect; do
    shells="${shells//[[:space:]]/}"
    [[ -z "$shells" || "$shells" == \#* ]] && continue
    expect=$(echo "$expect" | xargs)
    # The line as written: a trailing space means "complete the next word",
    # none means the last word is being typed.
    line="${line#"${line%%[! ]*}"}"
    [[ "$line" == *" " ]] && line="${line%%+( )} "
    for shell in bash zsh fish; do
        [[ "$shells" == all || ",$shells," == *",$shell,"* ]] || continue
        total=$((total + 1))
        t0=$(date +%s%N)
        # fish completes `--json=` to `--json=full`: count the value too.
        offered=$("complete_$shell" "$line" 2>/dev/null | sed 's/=$//' | sed -E 'p; s/^[^=]+=(.+)$/\1/' | sort -u)
        spent[$shell]=$(( spent[$shell] + $(date +%s%N) - t0 ))
        for e in $expect; do
            word=${e:1}
            case "$e" in
                +*) grep -qx -- "$word" <<< "$offered" || { echo "FAIL $shell: '$line' should offer $word"; fail=1; } ;;
                -*) grep -qx -- "$word" <<< "$offered" && { echo "FAIL $shell: '$line' must not offer $word"; fail=1; } ;;
            esac
        done
        if [[ $fail == 1 ]]; then
            echo "  offered: $(echo "$offered" | tr '\n' ' ')"
            exit 1
        fi
    done
done < cases.txt
secs() { echo "$(( $1 / 1000000000 )).$(( $1 / 100000000 % 10 ))s"; }
echo "completions: $total cases passed in $(secs $(( $(date +%s%N) - started ))) (bash $(secs ${spent[bash]}), zsh $(secs ${spent[zsh]}), fish $(secs ${spent[fish]}))"
