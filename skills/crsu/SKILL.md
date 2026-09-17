---
name: crsu
description: >-
  Drive Git + Crucible reviews with the crsu CLI: diff to create or update a
  review, land to pull-rebase and push the same branch, comments as stable
  JSON, patches to drop stale full uploads. Use when the user mentions crsu,
  crsu diff, crsu land, 出评审, 合入评审, 评审评论 JSON, Need Resolve, 标待解决,
  defect, patches, or prune.
---

# crsu

当前仓库里走 Git + Crucible 评审时用 `crsu`。
命令细节以 `crsu --help` / `crsu comments --help` / `crsu patches --help` 为准，不要凭记忆补旗标。

没有本地仓库、只读别人的评审页时，不要对评审做写操作。

## 分工

`diff` 出评审。基线要新，就 fetch `origin` 上的目标分支。不要为了 diff 去改本地那份 `master`/`main`。

`land` 合入。先对当前分支 `pull --rebase`，再 push。本地目标副本是 pull 的事；另一条本地 `master` 旧着或已经分叉，提一句即可，不挡正常 push。

`comments` 给 agent 用。stdout 是稳定 JSON。改自己的评论不必 `--i-mean-it`。

`patches` 清过往全量 patch。`diff` 只追加，不会自动删旧的。

## 出评审

工作区干净。基线写合入目标，例如 `origin/master`，不要写当前 feature 自己的 upstream。非交互加 `-y`。

成功后看 stdout 的 Review 行，以及 HEAD 提交里的 `Url:`。剪贴板摘要用 `crsu copy`，不要为了抄摘要去 fetch。

## 合入

`crsu land -y`。首版只做同分支、单个 commit。评审里的 `[ target: ]` 必须和即将 push 的分支一致，对不上要用 `--force`，不要假装跨分支 merge。还没人 complete 就不要 land。冲突停下来，交给用户 rebase。

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

## 不要做

不要在 `copy` 时同步 git。不要因为本地目标分支分叉而中止 push。不要实现或建议跨分支 land。不要在 `diff` 里自动 prune。
