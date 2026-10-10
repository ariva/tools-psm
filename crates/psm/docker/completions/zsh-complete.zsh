#!/usr/bin/env zsh
# Prints what zsh would offer for the command line given, one per line.
# Completion only runs inside the line editor, so an interactive zsh is
# driven through a pseudo-terminal: set up compinit, type the line, press
# Tab, and read the screen back. Reading stops when the prompt is drawn
# again (a listing was printed) or when the output goes quiet (a single
# match was inserted in place, nothing listed), not after a fixed sleep.
zmodload zsh/zpty
setopt no_beep
local line=$1
local sentinel='%PROMPT%'

# Reads from the pty into REPLY until the output matches the glob in $1,
# or has been quiet for $2 ms; gives up after $3 ms.
read_until() {
    local pattern=$1 quiet_ms=$2 max_ms=$3
    local acc='' chunk waited=0 quiet=0
    while (( waited < max_ms )); do
        if zpty -rt z chunk 2>/dev/null; then
            acc+=$chunk
            quiet=0
            [[ -n $pattern && $acc == ${~pattern} ]] && break
        else
            sleep 0.02
            (( waited += 20, quiet += 20 ))
            (( quiet >= quiet_ms )) && [[ -n $acc ]] && break
        fi
    done
    REPLY=$acc
}

zpty -b z 'zsh -f -i'
zpty -w z "PS1='$sentinel '; fpath=(/root/.zfunc \$fpath); autoload -Uz compinit; compinit -u"
zpty -w z 'zstyle ":completion:*" verbose no; zstyle ":completion:*" format ""; zstyle ":completion:*:descriptions" format ""; zstyle ":completion:*" list-separator "@@"; zstyle ":completion:*" menu no; setopt no_list_ambiguous no_auto_menu no_always_last_prompt no_beep; LISTMAX=0'
# Both setup lines have run once their prompts are back; then drop all of it.
read_until "*${sentinel}*${sentinel}*${sentinel}*" 200 5000
zpty -w -n z "${line}"$'\t'
# A listing ends with the prompt redrawn under it; an in-place insertion
# draws no new prompt, so the quiet timeout is what ends that case.
read_until "*${sentinel}*" 150 3000
local out=$REPLY
zpty -d z

out=${out//$'\r'/}
# Terminal control sequences the line editor emits (clear-to-end, cursor moves).
out=$(print -r -- "$out" | sed -E $'s/\x1b\\[[0-9;?]*[A-Za-z]//g; s/\x1b[()][A-Z0-9]//g')
local -a lines
lines=(${(f)out})
local l w
local typed=${line%% #}
local inserted=""
for l in $lines; do
    if [[ $l == *$typed* ]]; then
        # The command line itself, as echoed or as redrawn after the Tab.
        # A single match is inserted instead of listed: what follows the
        # typed text, joined to the word being typed, is that match.
        local rest=${l##*$typed}
        rest=${rest## #}
        if [[ -n $rest ]]; then
            local tail=${rest%% *}
            if [[ $line == *' ' ]]; then
                inserted=$tail
            else
                inserted=${typed##* }$tail
            fi
        fi
        continue
    fi
    [[ $l == *$sentinel* ]] && continue
    for w in ${(z)l}; do
        [[ $w == '@@'* ]] && continue
        print -- $w
    done
done
[[ -n $inserted ]] && print -- $inserted
