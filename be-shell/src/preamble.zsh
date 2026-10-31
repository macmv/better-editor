typeset +x BE_LINE BE_OUT BE_SCRIPT BE_BASELINE
[[ -r $BE_SCRIPT ]] && source $BE_SCRIPT 2>/dev/null
eval "$BE_LINE"
