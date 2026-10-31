# Defines __be_dump and arranges for it to run when the shell exits.
#
# Expects $BE_OUT to name an (existing) output directory. Writes cwd, env, and
# script into it, then writes `done` last. Everything is silenced, and the
# function never changes the exit status of the shell.
__be_dump() {
  # Capture the user's options before `emulate -L` resets them for this function.
  local -A __be_opts
  __be_opts=("${(@kv)options}")
  local -a __be_lines
  emulate -L zsh
  setopt extendedglob
  local __be_d=$BE_OUT __be_n __be_t __be_line
  local -a __be_names
  local -A __be_base
  [[ -n $__be_d && -d $__be_d ]] || return
  {
    print -rn -- $PWD >| $__be_d/cwd
    # Exported variables, NUL separated (cheaper than forking `env -0`).
    __be_lines=()
    for __be_n in ${(k)parameters[(R)*export*]}; do
      __be_lines+=("$__be_n=${(P)__be_n}")
    done
    (( ${#__be_lines} )) && print -rN -- "${__be_lines[@]}" >| $__be_d/env || : >| $__be_d/env
    # Baseline: what a pristine `zsh -f` has. Anything identical to it is
    # skipped, which removes zsh's own non-special builtin parameters
    # (ZSH_VERSION, CPUTYPE, LISTMAX, ...).
    # The baseline is cached in $BE_BASELINE (if set) to save a fork.
    if [[ -n $BE_BASELINE && -r $BE_BASELINE ]]; then
      __be_lines=("${(@f)$(<$BE_BASELINE)}")
    else
      __be_lines=("${(@f)$(command zsh -f -c 'typeset -p' </dev/null)}")
      [[ -n $BE_BASELINE ]] && print -rl -- $__be_lines >| $BE_BASELINE
    fi
    for __be_line in $__be_lines; do
      __be_base[$__be_line]=1
    done
    for __be_n in ${(k)parameters}; do
      __be_t=${parameters[$__be_n]}
      # Exported vars travel through the env dump; specials cannot be restored.
      [[ $__be_t == *(special|export|hide|local)* ]] && continue
      case $__be_n in
        (__be_*|BE_LINE|BE_OUT|BE_SCRIPT|BE_BASELINE|_|argv|ARGC|ZSH_EXECUTION_STRING|ZSH_ARGZERO|ZSH_NAME|ZSH_SUBSHELL|MBEGIN|MEND|CURSOR|BUFFER|LBUFFER|RBUFFER)
          continue;;
        (zshexit_functions|precmd_functions|preexec_functions|periodic_functions|zshaddhistory_functions)
          continue;;
        (*[^a-zA-Z0-9_]*) continue;;
      esac
      __be_names+=($__be_n)
    done
    {
      print -r -- "# be-shell snapshot"
      print -r -- "umask $(umask)"
      # zsh keeps its own idea of the previous directory (`cd -`) that cannot
      # be assigned through $OLDPWD, so replay the last cd. -q skips chpwd hooks.
      # If the directory is gone, stay where the shell was started.
      print -r -- "__be_start=\$PWD; builtin cd -q -- ${(q)OLDPWD} 2>/dev/null; builtin cd -q -- ${(q)PWD} 2>/dev/null || builtin cd -q -- \$__be_start; unset __be_start"
      print -r -- "fpath=(${(q)fpath[@]})"
      print -r -- "cdpath=(${(q)cdpath[@]})"
      alias -L
      alias -L -g
      alias -L -s
      if (( ${#__be_names} )); then
        # Written to a file so `$(<file)` reads it without forking a subshell.
        typeset -p -- $__be_names >| $__be_d/typeset
        for __be_line in "${(@f)$(<$__be_d/typeset)}"; do
          (( ${+__be_base[${__be_line/typeset -g /typeset }]} )) || print -r -- $__be_line
        done
      fi
      print -r -- "dirstack=(${(q)dirstack[@]})"
      for __be_n __be_t in ${(kv)__be_opts}; do
        case $__be_n in
          (interactive|monitor|shinstdin|login|zle|usezle|privileged|restricted|singlecommand|emacs|vi|errexit|errreturn|exec|verbose|xtrace|sourcetrace|printexitvalue) continue;;
        esac
        [[ $__be_t == on ]] && print -r -- "setopt $__be_n" || print -r -- "unsetopt $__be_n"
      done
      __be_names=(${(k)functions[(I)^(__be_*|_*)]})
      (( ${#__be_names} )) && functions -- $__be_names
    } >| $__be_d/script
    print ok >| $__be_d/done
  } 2>/dev/null
}
zshexit_functions+=(__be_dump)
