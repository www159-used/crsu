_crsu_complete() {
  crsu complete "$@" 2>/dev/null
}

_crsu() {
  local cur prev cmd
  COMPREPLY=()
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"
  cmd="${COMP_WORDS[1]}"

  if [[ ${COMP_CWORD} -eq 1 ]]; then
    COMPREPLY=($(compgen -W "init config cfg doctor doc status st diff df copy completions comp land ld comments cmt patches pt help" -- "${cur}"))
    return
  fi

  case "${cmd}" in
    init)
      COMPREPLY=($(compgen -W "-g --global -h --help" -- "${cur}"))
      ;;
    config|cfg)
      if [[ ${COMP_CWORD} -eq 2 ]]; then
        COMPREPLY=($(compgen -W "show set unset reviewer -g --global -h --help" -- "${cur}"))
      elif [[ ${prev} == set ]]; then
        COMPREPLY=($(compgen -W "$(_crsu_complete config-keys)" -- "${cur}"))
      elif [[ ${prev} == unset ]]; then
        COMPREPLY=($(compgen -W "repository" -- "${cur}"))
      elif [[ ${prev} == reviewer ]]; then
        COMPREPLY=($(compgen -W "list add remove" -- "${cur}"))
      else
        COMPREPLY=($(compgen -W "-g --global -h --help" -- "${cur}"))
      fi
      ;;
    status|st)
      COMPREPLY=($(compgen -W "$(_crsu_complete review-ids)" -- "${cur}"))
      ;;
    diff|df)
      case "${prev}" in
        -a|--attach)
          COMPREPLY=($(compgen -W "$(_crsu_complete review-ids)" -- "${cur}"))
          ;;
        *)
          if [[ ${cur} == -* ]]; then
            COMPREPLY=($(compgen -W "-a --attach -n --new -y --yes -f --force -h --help" -- "${cur}"))
          else
            COMPREPLY=($(compgen -W "$(_crsu_complete refs)" -- "${cur}"))
          fi
          ;;
      esac
      ;;
    copy)
      case "${prev}" in
        -j|--jira)
          COMPREPLY=($(compgen -W "$(_crsu_complete jira)" -- "${cur}"))
          ;;
        -b|--branches)
          COMPREPLY=($(compgen -W "$(_crsu_complete refs)" -- "${cur}"))
          ;;
        *)
          if [[ ${cur} == -* ]]; then
            COMPREPLY=($(compgen -W "-j --jira -b --branches -h --help" -- "${cur}"))
          else
            COMPREPLY=($(compgen -W "$(_crsu_complete refs)" -- "${cur}"))
          fi
          ;;
      esac
      ;;
    completions|comp)
      COMPREPLY=($(compgen -W "$(_crsu_complete shells)" -- "${cur}"))
      ;;
    land|ld)
      if [[ ${cur} == -* ]]; then
        COMPREPLY=($(compgen -W "-y --yes -f --force -h --help" -- "${cur}"))
      else
        COMPREPLY=($(compgen -W "$(_crsu_complete refs)" -- "${cur}"))
      fi
      ;;
    comments|cmt)
      if [[ ${COMP_CWORD} -eq 2 ]]; then
        COMPREPLY=($(compgen -W "list ls reply resolve delete rm edit defect undefect unresolve" -- "${cur}"))
      else
        case "${prev}" in
          -r|--review)
            COMPREPLY=($(compgen -W "$(_crsu_complete review-ids)" -- "${cur}"))
            ;;
          -m|--message)
            COMPREPLY=()
            ;;
          list|ls)
            COMPREPLY=($(compgen -W "$(_crsu_complete review-ids)" -- "${cur}"))
            ;;
          *)
            if [[ ${cur} == -* ]]; then
              COMPREPLY=($(compgen -W "-r --review -m --message -a --all -h --help" -- "${cur}"))
            else
              COMPREPLY=($(compgen -W "$(_crsu_complete comment-ids)" -- "${cur}"))
            fi
            ;;
        esac
      fi
      ;;
    patches|pt)
      if [[ ${COMP_CWORD} -eq 2 ]]; then
        COMPREPLY=($(compgen -W "list ls delete prune" -- "${cur}"))
      else
        case "${prev}" in
          -r|--review)
            COMPREPLY=($(compgen -W "$(_crsu_complete review-ids)" -- "${cur}"))
            ;;
          list|ls)
            COMPREPLY=($(compgen -W "$(_crsu_complete review-ids)" -- "${cur}"))
            ;;
          *)
            if [[ ${cur} == -* ]]; then
              COMPREPLY=($(compgen -W "-r --review -h --help" -- "${cur}"))
            else
              COMPREPLY=($(compgen -W "$(_crsu_complete patch-ids)" -- "${cur}"))
            fi
            ;;
        esac
      fi
      ;;
  esac
}

complete -F _crsu crsu
