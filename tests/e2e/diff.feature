功能: 准备代码评审差异

  为了将当前分支的已提交修改提交到代码评审系统
  作为开发者
  我希望 crsu diff 基于指定基线生成可复现的待审差异

  场景: 干净的功能分支可以生成待审差异
    假如 Git 仓库存在 main 分支，且 README.md 的内容为 "base"
    并且 当前分支 feature 相对于 main 有一个已提交的 README.md 修改
    并且 工作区没有任何未提交修改
    当 我在 feature 分支执行 "crsu diff main"
    那么 命令执行成功
    并且 输出显示基线为 "main"
    并且 输出显示待审提交数为 "1"
    并且 输出显示非零的 patch 字节数

  场景: 存在未提交文件时拒绝生成待审差异
    假如 Git 仓库存在 main 分支，且当前分支 feature 有一个已提交修改
    并且 工作区存在未提交的文件 "uncommitted.txt"
    当 我在 feature 分支执行 "crsu diff main"
    那么 命令执行失败
    并且 错误信息说明工作区存在未提交修改
