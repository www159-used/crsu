# crsu

`crsu` 是一个面向 Git 与 Crucible 代码评审流程的 Rust CLI。它是旧 Python
`cru` 的裁剪后重建项目，而不是兼容性移植。

项目使用 Cargo workspace：根包为 `crsu`，`crates/crsu-init` 承载初始化表单 model
与状态迁移测试。

许可证：GNU AGPL-3.0-or-later，见 [LICENSE](LICENSE)。

第一版只包含三个命令：

```text
crsu doctor
crsu diff [base]
crsu land [target]
```

- `doctor`：只读检查当前 Git 仓库。
- `init`：分段收集 Crucible 地址、用户名/密码、项目和 FishEye 仓库。它登录取得 token，
  不保存密码。项目与仓库候选从 Crucible 实时读取；有 `fzf` 时可搜索选择，没有则降级为
  编号选择。结果写入共享 Git 目录的 `.git/crsu/config.toml`（权限 `0600`）。
- `diff`：基于可选基线生成 patch；优先读取环境变量，其次读取 `.git/crsu/config.toml`
  创建 Crucible review。
- `land`：后续用于将当前分支合入可选目标分支。

`diff` 与 `land` 当前只完成命令接口，明确拒绝执行，避免在工作流和安全规则
确定前改动 Git、Crucible 或远端分支。

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

第一版不迁移 Jira、Jenkins、发布、tag、release note、cherry-pick/rebase、剪贴板，
以及 Hg/SVN/CVS/P4 支持。
