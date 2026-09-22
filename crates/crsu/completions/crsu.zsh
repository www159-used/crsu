#compdef crsu

_crsu_values() {
  local -a values
  values=(${(f)"$(_call_program crsu-complete crsu complete "$1" 2>/dev/null)"})
  (( ${#values} )) && _describe -t "$1" "$1" values
}

_crsu() {
  local context state state_descr line
  typeset -A opt_args
  local -a commands
  commands=(
    'init:交互式配置仓库'
    'config:查看或修改配置'
    'cfg:查看或修改配置'
    'doctor:检查本地 Git 环境'
    'doc:检查本地 Git 环境'
    'status:读取评审状态'
    'st:读取评审状态'
    'diff:创建或更新评审'
    'df:创建或更新评审'
    'copy:复制评审摘要'
    'completions:打印补全脚本'
    'comp:打印补全脚本'
    'land:合入评审'
    'ld:合入评审'
    'comments:评审评论'
    'cmt:评审评论'
    'patches:评审 patch'
    'pt:评审 patch'
  )
  _arguments -C \
    '(-h --help)'{-h,--help}'[Print help]' \
    '(-V --version)'{-V,--version}'[Print version]' \
    '1: :->command' \
    '*:: :->args'
  case $state in
    command)
      _describe -t commands 'command' commands
      ;;
    args)
      case $words[1] in
        init)
          _arguments '(-g --global)'{-g,--global}'[write user config]'
          ;;
        config|cfg)
          _crsu_config
          ;;
        status|st)
          _crsu_values review-ids
          ;;
        diff|df)
          _arguments \
            '(-a --attach)'{-a,--attach}'[attach review]:review:->reviews' \
            '(-n --new)'{-n,--new}'[force new review]' \
            '(-y --yes)'{-y,--yes}'[skip confirm]' \
            '(-f --force)'{-f,--force}'[allow large patch]' \
            '1:base:->refs'
          case $state in
            reviews) _crsu_values review-ids ;;
            refs) _crsu_values refs ;;
          esac
          ;;
        copy)
          _arguments \
            '(-j --jira)'{-j,--jira}'[JIRA key]:jira:->jira' \
            '(-b --branches)'{-b,--branches}'[refs]:ref:->refs' \
            '1:base:->refs'
          case $state in
            jira) _crsu_values jira ;;
            refs) _crsu_values refs ;;
          esac
          ;;
        completions|comp)
          _crsu_values shells
          ;;
        land|ld)
          _arguments \
            '(-y --yes)'{-y,--yes}'[skip confirm]' \
            '(-f --force)'{-f,--force}'[override target check]' \
            '1:target:->refs'
          [[ $state == refs ]] && _crsu_values refs
          ;;
        comments|cmt)
          _crsu_comments
          ;;
        patches|pt)
          _crsu_patches
          ;;
      esac
      ;;
  esac
}

_crsu_config() {
  if (( CURRENT == 2 )); then
    _values 'config' show set unset reviewer
    return
  fi
  case $words[2] in
    set)
      _arguments \
        '(-g --global)'{-g,--global}'[user config]' \
        '1:key:->keys'
      [[ $state == keys ]] && _crsu_values config-keys
      ;;
    unset)
      _values 'key' repository
      ;;
    reviewer)
      _values 'reviewer' list add remove
      ;;
    *)
      _arguments '(-g --global)'{-g,--global}'[user config]'
      ;;
  esac
}

_crsu_comments() {
  if (( CURRENT == 2 )); then
    _values 'comments' list ls reply resolve delete rm edit update defect undefect unresolve
    return
  fi
  case $words[2] in
    list|ls)
      _crsu_values review-ids
      ;;
    reply)
      _arguments \
        '(-m --message)'{-m,--message}'[message]:message:' \
        '(-r --review)'{-r,--review}'[review]:review:->reviews' \
        '1:comment:->comments'
      ;;
    edit|update)
      _arguments \
        '(-m --message)'{-m,--message}'[message]:message:' \
        '(-r --review)'{-r,--review}'[review]:review:->reviews' \
        '1:comment:->comments'
      ;;
    delete|rm)
      _arguments \
        '(-r --review)'{-r,--review}'[review]:review:->reviews' \
        '1:comment:->comments'
      ;;
    resolve|unresolve|defect|undefect|mark-resolved|needs-resolve|raise-defect|clear-defect)
      _arguments \
        '(-r --review)'{-r,--review}'[review]:review:->reviews' \
        '(-a --all)'{-a,--all}'[all top-level comments]' \
        '*:comment:->comments'
      ;;
  esac
  case $state in
    reviews) _crsu_values review-ids ;;
    comments) _crsu_values comment-ids ;;
  esac
}

_crsu_patches() {
  if (( CURRENT == 2 )); then
    _values 'patches' list ls delete prune
    return
  fi
  case $words[2] in
    list|ls)
      _crsu_values review-ids
      ;;
    delete)
      _arguments \
        '(-r --review)'{-r,--review}'[review]:review:->reviews' \
        '*:patch:->patches'
      ;;
    prune)
      _arguments '(-r --review)'{-r,--review}'[review]:review:->reviews'
      ;;
  esac
  case $state in
    reviews) _crsu_values review-ids ;;
    patches) _crsu_values patch-ids ;;
  esac
}

# fpath 里的 #compdef 文件只定义函数；不要在启动时调用 _arguments。
if [[ ${funcstack[1]} == _crsu ]]; then
  _crsu "$@"
fi
