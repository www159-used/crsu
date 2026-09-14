# crsu

`crsu` 是一个面向 Git 与 Crucible 代码评审流程的 Rust CLI。它是旧 Python
`cru` 的裁剪后重建项目，而不是兼容性移植。

许可证：GNU AGPL-3.0-or-later，见 [LICENSE](LICENSE)。

第一版只包含三个命令：

```text
crsu doctor
crsu diff [base]
crsu land [target]
```

- `doctor`：只读检查当前 Git 仓库。
- `diff`：后续用于以可选基线分支创建或更新 Crucible review。
- `land`：后续用于将当前分支合入可选目标分支。

`diff` 与 `land` 当前只完成命令接口，明确拒绝执行，避免在工作流和安全规则
确定前改动 Git、Crucible 或远端分支。

## 开发

```bash
cargo test
cargo run -- doctor
```

人类可读的端到端验收场景在 `tests/e2e/*.feature`；对应的 Rust 可执行绑定在
`tests/e2e_*.rs`。

## 非目标

第一版不迁移 Jira、Jenkins、发布、tag、release note、cherry-pick/rebase、剪贴板，
以及 Hg/SVN/CVS/P4 支持。
