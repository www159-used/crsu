function __crsu_complete
    command crsu complete $argv 2>/dev/null
end

complete -c crsu -f
complete -c crsu -s h -l help
complete -c crsu -s V -l version

complete -c crsu -n __fish_use_subcommand -a init -d '交互式配置仓库'
complete -c crsu -n __fish_use_subcommand -a 'config cfg' -d '查看或修改配置'
complete -c crsu -n __fish_use_subcommand -a 'doctor doc' -d '检查本地 Git 环境'
complete -c crsu -n __fish_use_subcommand -a 'status st' -d '读取评审状态'
complete -c crsu -n __fish_use_subcommand -a 'diff df' -d '创建或更新评审'
complete -c crsu -n __fish_use_subcommand -a copy -d '复制评审摘要'
complete -c crsu -n __fish_use_subcommand -a 'completions comp' -d '打印补全脚本'
complete -c crsu -n __fish_use_subcommand -a 'land ld' -d '合入评审'
complete -c crsu -n __fish_use_subcommand -a 'comments cmt' -d '评审评论'
complete -c crsu -n __fish_use_subcommand -a 'patches pt' -d '评审 patch'

complete -c crsu -n '__fish_seen_subcommand_from init config cfg' -s g -l global

complete -c crsu -n '__fish_seen_subcommand_from config cfg; and __fish_is_nth_token 2' -a 'show set unset reviewer'
complete -c crsu -n '__fish_seen_subcommand_from config cfg; and __fish_seen_subcommand_from set' -a '(__crsu_complete config-keys)'
complete -c crsu -n '__fish_seen_subcommand_from config cfg; and __fish_seen_subcommand_from unset' -a repository
complete -c crsu -n '__fish_seen_subcommand_from config cfg; and __fish_seen_subcommand_from reviewer' -a 'list add remove'

complete -c crsu -n '__fish_seen_subcommand_from status st' -a '(__crsu_complete review-ids)'

complete -c crsu -n '__fish_seen_subcommand_from diff df' -s a -l attach -xa '(__crsu_complete review-ids)'
complete -c crsu -n '__fish_seen_subcommand_from diff df' -s n -l new
complete -c crsu -n '__fish_seen_subcommand_from diff df' -s y -l yes
complete -c crsu -n '__fish_seen_subcommand_from diff df' -s f -l force
complete -c crsu -n '__fish_seen_subcommand_from diff df' -a '(__crsu_complete refs)'

complete -c crsu -n '__fish_seen_subcommand_from copy' -s j -l jira -xa '(__crsu_complete jira)'
complete -c crsu -n '__fish_seen_subcommand_from copy' -s b -l branches -xa '(__crsu_complete refs)'
complete -c crsu -n '__fish_seen_subcommand_from copy' -a '(__crsu_complete refs)'

complete -c crsu -n '__fish_seen_subcommand_from completions comp' -a '(__crsu_complete shells)'

complete -c crsu -n '__fish_seen_subcommand_from land ld' -s y -l yes
complete -c crsu -n '__fish_seen_subcommand_from land ld' -s f -l force
complete -c crsu -n '__fish_seen_subcommand_from land ld' -a '(__crsu_complete refs)'

complete -c crsu -n '__fish_seen_subcommand_from comments cmt; and __fish_is_nth_token 2' -a 'list ls reply resolve delete rm edit update defect undefect unresolve'
complete -c crsu -n '__fish_seen_subcommand_from comments cmt' -s r -l review -xa '(__crsu_complete review-ids)'
complete -c crsu -n '__fish_seen_subcommand_from comments cmt' -s m -l message
complete -c crsu -n '__fish_seen_subcommand_from comments cmt' -s a -l all
complete -c crsu -n '__fish_seen_subcommand_from comments cmt; and __fish_seen_subcommand_from list ls' -a '(__crsu_complete review-ids)'
complete -c crsu -n '__fish_seen_subcommand_from comments cmt; and __fish_seen_subcommand_from reply resolve delete rm edit update defect undefect unresolve' -a '(__crsu_complete comment-ids)'

complete -c crsu -n '__fish_seen_subcommand_from patches pt; and __fish_is_nth_token 2' -a 'list ls delete prune'
complete -c crsu -n '__fish_seen_subcommand_from patches pt' -s r -l review -xa '(__crsu_complete review-ids)'
complete -c crsu -n '__fish_seen_subcommand_from patches pt; and __fish_seen_subcommand_from list ls' -a '(__crsu_complete review-ids)'
complete -c crsu -n '__fish_seen_subcommand_from patches pt; and __fish_seen_subcommand_from delete' -a '(__crsu_complete patch-ids)'
