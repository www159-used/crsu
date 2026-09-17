---
name: crsu
description: >-
  Drive Git + Crucible reviews with the crsu CLI: diff to create or update a
  review, land to pull-rebase and push the same branch, status as stable
  JSON for agents, comments as stable JSON, patches to drop stale full
  uploads. Use when the user mentions crsu, crsu diff, crsu land, crsu
  status, 出评审, 合入评审, 评审状态 JSON, 评审评论 JSON, Need Resolve,
  标待解决, defect, patches, or prune.
---

# crsu

当前仓库里走 Git + Crucible 评审时用 `crsu`。
命令细节以 `crsu --help` / `crsu status --help` / `crsu comments --help` / `crsu patches --help` 为准，不要凭记忆补旗标。

没有本地仓库、只读别人的评审页时，不要对评审做写操作。

## 分工

`diff` 出评审。基线要新，就 fetch `origin` 上的目标分支。不要为了 diff 去改本地那份 `master`/`main`。

`land` 合入。先对当前分支 `pull --rebase`，再 push。本地目标副本是 pull 的事；另一条本地 `master` 旧着或已经分叉，提一句即可，不挡正常 push。

`status` 只问 Crucible。stdout 是稳定 JSON：`state`、reviewers、objectives。git log / rebase / 脏工作区用 git 和 bash 做。批量 `diff`/`land` 不要做成一条 crsu 命令。

`comments` 给 agent 用。stdout 是稳定 JSON。改自己的评论不必 `--i-mean-it`。

`patches` 清过往全量 patch。`diff` 只追加，不会自动删旧的。

`pre-diff` / `pre-land` / `post-diff` / `post-land` 是 `.git/crsu/hooks/` 下的可执行文件。stdin 为 JSON。pre 非 0 则中止命令；post 在成功后跑，失败不回滚，用来关 agent session、清 zellij tab。没有 hook 就是空操作。

## 出评审

工作区干净。基线写合入目标，例如 `origin/master`，不要写当前 feature 自己的 upstream。非交互加 `-y`。

HEAD 里的 `Url:` 指向未关闭的评审时追加 patch；评审已关闭或已放弃则新建，并改写 `Url:`。

成功后看 stdout 的 Review 行，以及 HEAD 提交里的 `Url:`。当前 HEAD 摘要用 `crsu copy`。同一 JIRA 铺了多条分支时用 `crsu copy --jira TIC-xxxx`，只读各提交的 `Url:`；有 `[ target: ]` 用它，没有就用分支名（存在 `origin/<branch>` 则写成 `origin/<branch>`）。不要 checkout 或 fetch。stdout 和剪贴板都是那几行摘要。V22 不要走 `--jira`。

## 状态

```bash
crsu status                 # 默认从 HEAD 的 Url: 读 review id
crsu status LP-1476
crsu status LP-1476 LP-1478
```

顶层字段是 `reviews`。每条认这些键：`review_id`、`url`、`title`、`state`、`objectives`、`target`（从 objectives 的 `[ target: ]` 抽出）、`reviewers`（`username` / `completed`）。这是 `GET reviews-v1/{id}` 加 reviewers，不扫仓库、不算能不能 rebase。

各分支的 id 用 `copy --jira` 或 `git log --grep`。能不能 land 由上层看 `state` / `completed`，再加上自己的 `git merge-tree` / `git status`。

## 合入

`crsu land -y`。首版只做同分支、单个 commit。评审里的 `[ target: ]` 必须和即将 push 的分支一致，对不上要用 `--force`，不要假装跨分支 merge。还没人 complete 就不要 land。评审已关闭或已放弃不要 land，不要 rebase，不要 push。冲突停下来，交给用户 rebase。先 `status` 再决定要不要 land。

## 评论

```bash
crsu comments                  # 默认从 HEAD 的 Url: 读 review id
crsu comments list REVIEW_ID
```

顶层字段是 `review_id` 和 `comments`。每条评论认这些键：`id`、`kind`（`general` / `line`）、`author`、`message`、`draft`、`deleted`、`defect`、`path`、`line`、`created`、`replies`。行内评论才有 `path` / `line`。id 用 `CMT:39844` 这种形式。

回复、改、删、标状态：

```bash
crsu comments reply CMT:1 -m '正文'
crsu comments edit CMT:1 -m '新正文'
crsu comments delete CMT:1
crsu comments unresolve CMT:1
crsu comments resolve CMT:1
crsu comments defect CMT:1
crsu comments undefect CMT:1
```

只能改或删自己的评论。省略 review id 时同样从 HEAD 的 `Url:` 读。写完再 `list` 一次，以新的 JSON 为准。

## Patch

```bash
crsu patches                   # 默认从 HEAD 的 Url: 读 review id
crsu patches list REVIEW_ID
crsu patches delete 37473
crsu patches prune
```

list 只有元数据：`id`、`source`、`file`、`uploaded`、`comments`、`latest`。正文不输出。`delete` / `prune` 返回 `deleted`、`kept`、`skipped`。`skipped.reason` 为 `has_comments` 时整次仍成功。id 用 `37473` 或 `PATCH:37473`。

## Hook

可执行文件：`.git/crsu/hooks/pre-diff`、`post-diff`、`pre-land`、`post-land`（共享 git 目录，linked worktree 也能看到）。stdin 一段 JSON。

```json
{"version":1,"event":"post-land","command":"land","review_id":"LP-1478","url":"http://crucible/cru/LP-1478","branch":"feature","target":"origin/feature"}
```

`version` 是 hook 协议版本。多出来的键可以忽略；`version` 升了再按新合同解析。只在字段改义或删除时升版本。

`pre-*` 在提交评审或 rebase/push 之前；非 0 退出则命令失败。`post-*` 只在成功后跑，失败只警告，用来删 agent session、清 zellij tab。不要用 hook 代替 git/bash 做扫描或 rebase。

## 不要做

不要在 `copy` 时同步 git。不要让 crsu 去做 git/bash 能做的事（扫分支、merge-tree、判断脏工作区）。不要因为本地目标分支分叉而中止 push。不要实现或建议跨分支 land。不要实现或建议批量 `diff --jira` / `land --jira`。不要在 `diff` 里自动 prune。
