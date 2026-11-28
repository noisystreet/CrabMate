[
        builtin_tool! {
            name: tool_names::DELEGATE_TASK,
            args: args::DelegateTaskArgs,
            category: Development,
            tags: [dev_tag::GENERAL],
            runner: runner_delegate_task_try,
            summary: static "delegate task",
            description: "派一个**有界只读子任务**，子任务用独立消息窗与预算，只回传有界摘要与关键路径。\n\n【何时用】探索面大、需要隔离上下文、或想知道「某块代码/目录大概是什么」而不想把工具日志灌进本回合。**单次读一个文件、单次检索不要用**——那样只会放大成本。\n\n【子任务能做什么】只能用只读工具子集（按注册表派生，写工具放不进来）；权限取父回合的 `min` 链结果，不会更高；超预算/取消只体现为一条 `budget_exhausted` 状态的 report，**不会中断本回合**。\n\n【参数】只需 `prompt`：子任务目标。父历史、工具日志、system 全文都不会传过去。\n\n默认开启（置 `subagent_enabled = false` 则本工具不出现）。",
        },
]
