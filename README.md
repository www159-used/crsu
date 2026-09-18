# crsu

`crsu` 是一个面向 Git 与 Crucible 代码评审流程的 Rust CLI。它是旧 Python
`cru` 的裁剪后重建项目，而不是兼容性移植。

项目使用 Cargo workspace：根包为 `crsu`，`crates/crsu-init` 承载初始化表单 model
与状态迁移测试。

许可证：GNU AGPL-3.0-or-later，见 [LICENSE](LICENSE)。

第一版只包含三个命令：

```text
crsu doctor
crsu status
crsu status LP-1476 LP-1478
crsu diff [base]
crsu copy [base]
crsu copy --jira TIC-xxxx
crsu land [target]
crsu comments
crsu comments list [REVIEW_ID]
crsu comments reply COMMENT_ID -m MESSAGE
crsu comments edit COMMENT_ID -m MESSAGE
crsu comments delete COMMENT_ID
crsu comments resolve COMMENT_ID
crsu comments unresolve COMMENT_ID
crsu comments defect COMMENT_ID
crsu comments undefect COMMENT_ID
crsu patches
crsu patches list [REVIEW_ID]
crsu patches delete PATCH_ID
crsu patches prune
```

- `doctor`：只读检查当前 Git 仓库。
- `status`：只读拉取 Crucible 评审（`GET reviews-v1/{id}` + reviewers）。默认用 HEAD 的 `Url:`；也可直接传一个或多个 review id。stdout 是稳定 JSON：`state`、`title`、`objectives`、`target`、`reviewers`。不扫 git、不算 rebase / landable。要找各分支的 id 用 `copy --jira` 或 `git log`。
- `init`：分段收集 Crucible 地址、用户名/密码、项目和 FishEye 仓库。它登录取得 token，
  不保存密码。项目与仓库候选从 Crucible 实时读取；有 `fzf` 时可搜索选择，没有则降级为
  编号选择。结果写入共享 Git 目录的 `.git/crsu/config.toml`（权限 `0600`）。
- `diff`：基于可选基线生成 patch；优先读取环境变量，其次读取 `.git/crsu/config.toml`
  创建 Crucible review。HEAD 里已有未关闭的评审则追加 patch；已关闭或已放弃则新建。
  摘要打印文件数和变更行数（`+`/`-`，不含文件头）；超过 1000 行拒绝提交，`--force` 才能越过。
- `copy`：输出 `[target] title url` 并写入剪贴板。默认当前 HEAD；`--jira` 按提交里的 `Url:` 聚合各分支，只读、不 checkout。没有 `[ target: ]` 时用分支名。
- `land`：将当前分支 rebase 到 upstream 后 push，并关闭已完成的 Crucible review（首版仅支持同分支、单个 commit；review 记录的 target 必须与即将 push 的分支一致，`-y` 跳过确认，`--force` 才能覆盖目标不一致）。评审已关闭或已放弃直接拒绝，不 rebase、不 push。
- 命令 hook：可执行文件放在 `$XDG_CONFIG_HOME/crsu/hooks/`（未设则 `~/.config/crsu/hooks/`）和共享 git 目录的 `.git/crsu/hooks/`。两层都跑，不互相覆盖。`pre-*` 先全局后项目，非 0 退出则中止。`post-*` 先项目后全局，失败只警告，给外部做收尾（关 session、清 tab）。stdin 是一段 JSON（`version`、`scope`、`event`、`command`、`review_id`、`url` 等）。`version` 现为 `1`，字段改义或删除时才加一。
- `comments`：从 Crucible 拉取或修改评审评论，stdout 输出稳定 JSON。省略 review id 时从 HEAD 的 `Url:` 读取。`reply` 回复一条评论；`edit` / `update` 改写自己的评论；`delete` / `rm` 删除自己的评论；`unresolve` / `needs-resolve` 标成 Needs resolution；`resolve` / `mark-resolved` 标成 Resolved；`defect` / `raise-defect` 标成缺陷；`undefect` / `clear-defect` 取消缺陷。Crucible 只允许改/删自己的评论。
- `patches`：列出或删除评审上的过往 patch（`-na` / `diff` 每次追加的全量）。`list` 只输出元数据；`delete` 删指定块；`prune` 只留最新。挂着未删行内评论的 patch 会跳过，不挡整次清理。`diff` 不会自动 prune。

`diff` 已可创建/更新评审；`land` 已支持同分支合入。跨分支 merge 尚未实现。

给 agent 的用法约束是一份通用 Agent Skill，见 [`skills/crsu/SKILL.md`](skills/crsu/SKILL.md)。需要时拷到所用 CLI 的 skills 目录即可，例如：

```bash
cp -R skills/crsu ~/.claude/skills/crsu
cp -R skills/crsu ~/.cursor/skills/crsu
```

## 开发

```bash
make check
cargo run -- doctor
cargo run -- init
```

常用开发入口：`make fmt`、`make test`、`make lint`、`make build`、`make install`。

## 安装

```bash
./scripts/install.sh
```

默认安装到 `${CARGO_HOME:-$HOME/.cargo}/bin/crsu`。若需要隔离安装目录：

```bash
./scripts/install.sh --root /path/to/install-root
```

声明式端到端场景在 `tests/e2e/**/*.yaml`；对应的 Rust runner 在
`tests/e2e_*.rs`。每个 YAML 用例声明 Git 初始状态、待执行的命令和预期输出。

## 非目标

第一版不迁移 Jira、Jenkins、发布、tag、release note、cherry-pick/rebase，
以及 Hg/SVN/CVS/P4 支持。
