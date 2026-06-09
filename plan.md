下面是一份按你前面要求整理的 **V1 / V2 详细开发 Plan**。核心目标保持不变：

> 使用 Rust 编写一个 Windows 端高性能、低资源占用的后台日程提醒 App。
> 日程提醒在规定日期和时段内及时触发，并以文字从屏幕滚动飘过的方式提醒。
> 时段规则要足够自由，例如支持固定时间、时间窗口、固定间隔循环提醒等。

------

# 一、总体技术判断

## 1. 不建议第一版直接做成纯 Windows Service

Windows Service 不适合直接显示桌面 UI。提醒文字需要出现在用户当前桌面上，所以真正负责飘字的进程应该是 **用户登录会话内的后台 Agent**。

推荐路线：

```text
V1：
reminder-agent.exe
单用户后台进程
登录后自动启动
负责调度 + 显示 + 本地存储

V2：
reminder-agent.exe
+
设置界面 / 托盘 / 多显示器 / 勿扰模式 / 高级规则
+
可选 reminder-service.exe，但不作为必须项
```

第一版重点是把核心能力打牢：**低功耗调度、复杂时段规则、稳定飘字显示、可靠本地存储**。

------

# 二、V1 开发目标

## V1 定位

V1 是一个可长期后台运行的轻量版本。

它不追求复杂 UI，而是优先验证核心闭环：

```text
创建提醒规则
↓
后台低功耗等待
↓
到点触发
↓
屏幕飘字显示
↓
计算下一次提醒
↓
继续等待
```

------

# 三、V1 功能范围

## 1. 必须支持的提醒规则

### 固定日期时间提醒

例如：

```text
2026-07-01 09:30 提醒我开会
```

### 每日固定时间提醒

例如：

```text
每天 09:00 提醒喝水
每天 18:00 提醒下班复盘
```

### 每周固定时间提醒

例如：

```text
每周一、三、五 10:00 提醒运动
```

### 时间窗口 + 固定间隔提醒

这是你特别强调的核心能力。

例如：

```text
每天 09:00 ~ 18:00，每 30 分钟提醒一次
工作日 10:00 ~ 12:00，每 15 分钟提醒一次
每天 22:00 ~ 23:00，每 5 分钟提醒一次
```

### 指定日期范围内循环提醒

例如：

```text
2026-07-01 到 2026-07-31
每天 09:00 ~ 18:00
每 25 分钟提醒一次
```

------

## 2. V1 暂不做的功能

为了第一版稳定，以下功能放到 V2 或以后：

```text
云同步
账号系统
复杂协作日历
自然语言解析
移动端同步
企业级策略下发
完整主题商城
远程推送
```

------

# 四、V1 技术架构

## 1. 进程结构

```text
reminder-agent.exe
 ├─ App Core
 │   ├─ 初始化
 │   ├─ 配置加载
 │   ├─ 日志系统
 │   └─ 生命周期管理
 │
 ├─ Storage Layer
 │   ├─ SQLite 存储
 │   ├─ 数据迁移
 │   └─ 配置读写
 │
 ├─ Schedule Engine
 │   ├─ 规则解析
 │   ├─ 下一次触发时间计算
 │   ├─ MinHeap 调度队列
 │   └─ 错过提醒处理
 │
 ├─ Timer Runtime
 │   ├─ 单 Waitable Timer
 │   ├─ 系统恢复后重算
 │   └─ 时间变更后重算
 │
 ├─ Overlay UI
 │   ├─ Win32 透明窗口
 │   ├─ 置顶显示
 │   ├─ 滚动文字动画
 │   └─ 多条提醒排队
 │
 └─ Local Control
     ├─ CLI 管理命令
     └─ 可选本地 IPC
```

------

## 2. 推荐技术栈

```text
语言：Rust

Windows API：
windows crate

本地存储：
SQLite

序列化：
serde

日志：
tracing 或 log

时间处理：
time 或 chrono
Windows 系统时间转换单独封装

UI / 飘字：
Win32 API
Layered Window
Topmost Window
Direct2D / DirectWrite

自动启动：
Windows Task Scheduler 登录触发
```

第一版不建议用 Electron、Tauri、WebView 常驻后台。它们会显著提高内存占用，不符合“极低资源占用”的目标。

------

# 五、V1 模块开发 Plan

## 阶段 1：工程骨架

目标：先建立一个稳定的 Rust Windows 桌面后台程序骨架。

### 任务

```text
1. 创建 Rust workspace
2. 拆分核心 crate
3. 初始化日志系统
4. 初始化配置目录
5. 初始化 SQLite 数据库
6. 增加基础错误处理
7. 增加 Windows 单实例锁
```

建议 workspace：

```text
reminder/
 ├─ crates/
 │   ├─ app-core/
 │   ├─ scheduler/
 │   ├─ storage/
 │   ├─ overlay/
 │   ├─ winrt/
 │   └─ cli/
 ├─ apps/
 │   └─ reminder-agent/
 └─ Cargo.toml
```

### 验收标准

```text
- reminder-agent.exe 可以启动
- 启动后不弹主窗口
- 日志能写入本地文件
- 数据库能自动创建
- 重复启动时不会出现多个后台实例
```

------

## 阶段 2：数据模型设计

目标：定义能够支持高自由度时段的提醒模型。

### 核心表

```text
reminders
 ├─ id
 ├─ title
 ├─ message
 ├─ enabled
 ├─ priority
 ├─ timezone
 ├─ schedule_json
 ├─ display_json
 ├─ created_at
 ├─ updated_at

reminder_runtime
 ├─ reminder_id
 ├─ next_fire_at_utc
 ├─ last_fire_at_utc
 ├─ missed_count
 ├─ updated_at

reminder_history
 ├─ id
 ├─ reminder_id
 ├─ fired_at_utc
 ├─ displayed_at_utc
 ├─ result
```

### schedule_json 结构

```text
ScheduleRule
 ├─ date_range
 │   ├─ start_date
 │   └─ end_date?
 │
 ├─ day_filter
 │   ├─ every_day
 │   ├─ weekdays
 │   ├─ selected_weekdays
 │   ├─ month_days
 │   └─ specific_dates
 │
 ├─ time_windows
 │   ├─ fixed_times
 │   └─ interval_windows
 │
 ├─ exclusions
 │   ├─ excluded_dates
 │   └─ excluded_time_ranges
 │
 └─ missed_policy
     ├─ skip
     ├─ fire_once
     └─ fire_all_limited
```

### display_json 结构

```text
DisplayPolicy
 ├─ duration_seconds
 ├─ speed_px_per_sec
 ├─ position
 ├─ font_size
 ├─ opacity
 ├─ click_through
 ├─ repeat_on_screen
 └─ lane_policy
```

### 验收标准

```text
- 可以存储一次性提醒
- 可以存储每日提醒
- 可以存储每周提醒
- 可以存储时间窗口 + 间隔提醒
- 可以读取并还原为 Rust 结构体
- 可以做数据库 schema migration
```

------

## 阶段 3：规则引擎

目标：实现最关键的算法：根据规则计算下一次提醒时间。

### 核心接口

```text
next_fire_after(rule, after_time) -> Option<DateTimeUtc>
```

### 必须覆盖的规则

```text
1. 一次性提醒
2. 每日固定时间
3. 每周固定时间
4. 时间窗口内固定间隔
5. 日期范围限制
6. 排除日期
7. 跨天窗口
8. 当前时间已经超过窗口时，跳到下一个可用日期
```

### 时间窗口算法

对于：

```text
09:00 ~ 18:00，每 25 分钟一次
```

如果当前时间是：

```text
10:13
```

计算：

```text
elapsed = 10:13 - 09:00
k = ceil(elapsed / 25min)
candidate = 09:00 + k * 25min
```

结果：

```text
10:15
```

如果 candidate 超过 18:00，则进入下一个有效日期。

### 跨天窗口处理

例如：

```text
22:00 ~ 02:00，每 30 分钟提醒一次
```

这不能简单按同一天处理。应该拆成两个逻辑段：

```text
当天 22:00 ~ 24:00
次日 00:00 ~ 02:00
```

或者内部统一用“窗口起始日期 + offset minutes”建模。

### 验收标准

```text
- 单元测试覆盖主要规则
- 任意规则都能返回下一次触发时间
- 已过期的一次性提醒返回 None
- 日期范围结束后的规则返回 None
- 时间窗口内 interval 计算准确
- 休眠恢复后可以正确计算下一次
```

------

## 阶段 4：低功耗调度器

目标：实现后台资源占用极低的提醒调度。

### 核心算法

```text
1. 启动时加载所有 enabled reminders
2. 对每个 reminder 计算 next_fire_at
3. 放入 MinHeap
4. 取堆顶时间
5. 设置一个 Waitable Timer
6. 等待 timer 触发
7. 触发后弹出所有到期 reminder
8. 推送给 Overlay
9. 重新计算这些 reminder 的 next_fire_at
10. 放回 MinHeap
11. 重设 Waitable Timer
```

### 为什么用 MinHeap

```text
提醒数量很多时，不需要每秒扫描所有提醒。
只需要等待最近的一条。
```

复杂度：

```text
新增提醒：O(log n)
修改提醒：O(log n)
触发提醒：O(log n)
查看最近提醒：O(1)
空闲 CPU：接近 0
```

### 系统事件处理

V1 至少需要监听：

```text
系统从睡眠恢复
系统时间变化
时区变化
用户锁屏 / 解锁，可选
显示器变化，可选
```

发生这些事件后：

```text
清空调度堆
重新加载 enabled reminders
重新计算 next_fire_at
重新设置 Waitable Timer
```

### 验收标准

```text
- 后台空闲时 CPU 近似 0
- 不使用每秒轮询
- 只维护一个系统级定时等待点
- 修改提醒后能重建调度队列
- 系统睡眠恢复后不会漏掉逻辑
```

------

## 阶段 5：飘字 Overlay

目标：实现从屏幕滚动飘过的提醒效果。

### 窗口要求

```text
- 无边框
- 不显示在任务栏
- 透明背景
- 置顶
- 可选鼠标穿透
- 平时隐藏
- 仅提醒期间显示
```

### Win32 样式建议

```text
WS_POPUP
WS_EX_LAYERED
WS_EX_TOPMOST
WS_EX_TOOLWINDOW
WS_EX_TRANSPARENT，可选
```

### 渲染方式

V1 推荐：

```text
Direct2D + DirectWrite
```

如果第一阶段想更快验证，也可以先用 GDI / GDI+，但最终建议切到 Direct2D / DirectWrite。

### 动画模型

```text
x(t) = screen_width - speed_px_per_sec * elapsed
y = lane_y
```

当：

```text
x + text_width < 0
```

说明文字完全离开屏幕，动画结束。

### 多条提醒处理

使用 lane 机制：

```text
lane 0：顶部第一行
lane 1：顶部第二行
lane 2：顶部第三行
```

分配规则：

```text
1. 找空闲 lane
2. 没有空闲 lane 时进入显示队列
3. 高优先级提醒可以插队
4. 队列过长时合并展示
```

例如：

```text
你有 3 条提醒：喝水、会议、拉伸
```

### 验收标准

```text
- 到点后文字能从右向左滚动
- 透明窗口不遮挡正常桌面背景
- 可配置是否鼠标穿透
- 多条提醒不会完全重叠
- 提醒结束后窗口隐藏
- 空闲时不持续渲染
```

------

## 阶段 6：本地管理入口

V1 可以先不做完整 GUI，但需要有基本管理方式。

推荐做一个 CLI：

```text
reminder.exe add
reminder.exe list
reminder.exe enable
reminder.exe disable
reminder.exe delete
reminder.exe test-overlay
reminder.exe next
```

示例：

```text
reminder add --title "喝水" --every-day --window 09:00-18:00 --interval 30m
reminder list
reminder next
reminder test-overlay "这是一条测试提醒"
```

CLI 和 Agent 通信方式：

```text
V1 简单方案：
CLI 直接写 SQLite，然后通知 Agent reload

通知方式：
- Named Event
- Named Pipe
- 本地 TCP
- Windows Message
```

建议 V1 用：

```text
Named Event + SQLite
```

修改数据库后，CLI 触发一个 named event，Agent 收到后重载调度队列。

### 验收标准

```text
- 可以通过 CLI 新增提醒
- 可以查看提醒列表
- 可以禁用 / 启用提醒
- 可以删除提醒
- 可以手动测试飘字
- 修改提醒后 Agent 无需重启
```

------

## 阶段 7：启动与安装

目标：让程序能像正常后台提醒软件一样使用。

### V1 推荐方式

```text
使用 Windows Task Scheduler 注册登录启动任务
```

不要第一版就写 Windows Service。

### 安装内容

```text
- 拷贝 reminder-agent.exe
- 拷贝 reminder.exe CLI
- 创建数据目录
- 创建日志目录
- 注册登录启动任务
- 可选创建卸载脚本
```

### 验收标准

```text
- Windows 登录后自动启动
- 用户手动退出后不立即重启
- 卸载时可以清理启动任务
- 数据和日志位置明确
```

------

## 阶段 8：测试与性能验证

### 单元测试

重点测：

```text
- next_fire_after
- 时间窗口 interval
- 跨天窗口
- 日期范围
- 排除日期
- missed_policy
- MinHeap 调度顺序
```

### 集成测试

重点测：

```text
- 启动加载数据库
- 新增提醒后 reload
- 到点触发 overlay
- 多条提醒同时触发
- 休眠恢复后的补偿逻辑
```

### 性能指标

V1 建议设定目标：

```text
空闲 CPU：接近 0
空闲线程数：尽量少
空闲内存：保持轻量
提醒期间 CPU：可控，不持续飙高
数据库写入：仅在配置变更或触发历史记录时发生
```

不要把“空闲占用”做成模糊指标，开发时就应该用工具持续看：

```text
Task Manager
Process Explorer
Windows Performance Recorder
Windows Performance Analyzer
```

------

# 六、V1 交付清单

V1 完成时应具备：

```text
1. reminder-agent.exe
2. reminder.exe CLI
3. SQLite 本地数据库
4. 登录自动启动能力
5. 固定时间提醒
6. 每日 / 每周提醒
7. 时间窗口 + 固定间隔提醒
8. 透明置顶滚动飘字
9. 多条提醒队列
10. 休眠恢复处理
11. 基础日志
12. 基础测试用例
```

V1 不求漂亮，但必须稳定、轻、准。

------

# 七、V2 开发目标

## V2 定位

V2 是从“可用核心”升级为“完整桌面产品”。

重点是：

```text
更好的配置体验
更灵活的提醒规则
更强的显示控制
更稳定的后台运行
更好的异常恢复
更完整的用户体验
```

------

# 八、V2 功能范围

## 1. 图形化设置界面

V2 应增加一个设置 App：

```text
reminder-ui.exe
```

它不常驻。用户需要配置时打开，用完退出。

避免让 UI 框架长期占用资源。

可选技术：

```text
方案 A：原生 Win32 / egui / iced
方案 B：Tauri 只作为设置界面，不常驻
方案 C：Slint
```

不建议把 Agent 本身做成 WebView 常驻。

### 设置界面功能

```text
- 新建提醒
- 编辑提醒
- 删除提醒
- 启用 / 停用提醒
- 预览飘字效果
- 查看下一次提醒时间
- 查看提醒历史
- 配置勿扰时段
- 配置字体、速度、位置、透明度
```

------

## 2. 托盘图标

V2 增加系统托盘：

```text
- 当前运行状态
- 暂停提醒
- 恢复提醒
- 打开设置
- 测试提醒
- 查看下一条提醒
- 退出
```

托盘进程可以和 Agent 是同一个，也可以是 UI 进程。

推荐：

```text
Agent 负责托盘
UI 只在打开设置时启动
```

这样用户能感知程序正在运行，同时不需要常驻完整设置界面。

------

## 3. 更强的规则编辑器

V2 应支持更复杂的组合规则。

### 高级规则

```text
- 每 N 天
- 每 N 周
- 每月第几个星期几
- 每月固定日期
- 只在工作日
- 排除节假日
- 自定义例外日期
- 多个时间窗口
- 多个固定提醒时间
```

### 规则预览

这是 V2 很重要的体验。

用户创建规则后，界面应显示：

```text
接下来 10 次提醒时间：
1. 2026-07-01 09:00
2. 2026-07-01 09:30
3. 2026-07-01 10:00
...
```

这可以直接复用 V1 的 `next_fire_after`，连续调用多次得到结果。

------

## 4. 多显示器支持

V1 可以先只显示在主屏。

V2 要支持：

```text
- 只在主屏显示
- 在鼠标所在屏幕显示
- 在所有屏幕显示
- 指定某个显示器显示
```

多显示器策略：

```text
DisplayTarget
 ├─ primary
 ├─ active_cursor_monitor
 ├─ all_monitors
 └─ selected_monitor
```

多屏显示时有两个选择：

```text
方案 A：每个屏幕一个 overlay window
方案 B：创建覆盖虚拟桌面的单个 overlay window
```

推荐方案 A，因为每个显示器 DPI、缩放、坐标都可能不同。

------

## 5. 勿扰模式

V2 必须有，因为滚动飘字容易打断用户。

### 勿扰规则

```text
- 指定时间段勿扰
- 全屏应用时勿扰
- 演示模式勿扰
- 手动暂停 30 分钟
- 手动暂停到明天
```

### 勿扰期间的 missed_policy

```text
skip
勿扰期间直接跳过

fire_once_after_dnd
勿扰结束后只补一条

queue_limited
最多补 N 条
```

默认建议：

```text
fire_once_after_dnd
```

------

## 6. 显示体验增强

V2 的飘字体验可以增强：

```text
- 不同优先级不同样式
- 文字描边
- 阴影
- 背景胶囊
- 渐入渐出
- 鼠标悬停暂停
- 点击打开详情
- 快速延后提醒
- 快速完成提醒
```

### 提醒交互

```text
点击提醒：
  打开详情

右键提醒：
  延后 5 分钟
  延后 15 分钟
  今天不再提醒
  关闭本条
```

为了低资源，交互只在 overlay 显示期间启用。

------

## 7. Snooze 延后机制

V2 应支持延后提醒。

数据模型新增：

```text
snoozed_until_utc
snooze_count
```

处理逻辑：

```text
如果 reminder 有 snoozed_until：
  下一次提醒取 min(rule_next_fire, snoozed_until) 或优先 snooze
```

建议策略：

```text
延后提醒是一次性 runtime event
不修改原始 schedule rule
```

否则规则会变复杂。

------

## 8. 日志与诊断界面

V2 应该让用户知道为什么某条提醒没出现。

增加：

```text
- 最近触发历史
- 最近跳过原因
- 下一次提醒时间
- Agent 当前状态
- 数据库位置
- 日志导出
```

提醒历史可以记录：

```text
fired
displayed
skipped_by_dnd
skipped_expired
snoozed
failed_to_display
```

------

# 九、V2 架构升级

## V2 推荐进程结构

```text
reminder-agent.exe
 ├─ 常驻
 ├─ 调度
 ├─ 托盘
 ├─ Overlay
 ├─ IPC Server
 └─ 状态管理

reminder-ui.exe
 ├─ 设置界面
 ├─ 规则编辑器
 ├─ 历史查看
 └─ 预览配置

reminder.exe
 └─ CLI，可保留

可选：
reminder-service.exe
 ├─ 自动更新
 ├─ 企业策略
 └─ 多用户协调
```

V2 仍然不建议让 Windows Service 负责显示。Service 即使存在，也只是辅助。

------

## V2 IPC 方案

V1 可以用 Named Event + SQLite。

V2 建议升级为：

```text
Named Pipe
```

用途：

```text
UI 查询 Agent 状态
UI 通知 Agent 重载
UI 请求测试提醒
UI 请求暂停 / 恢复
CLI 操作 Agent
Agent 返回下一次提醒时间
```

IPC 消息示例：

```text
GetStatus
ReloadRules
ShowTestReminder
PauseForDuration
Resume
GetNextReminders
PreviewSchedule
```

------

# 十、V2 算法增强

## 1. 规则预览算法

```text
preview(rule, from_time, limit):
  result = []
  cursor = from_time

  while result.len < limit:
    next = next_fire_after(rule, cursor)
    if next is None:
      break

    result.push(next)
    cursor = next + 1ms

  return result
```

用途：

```text
- UI 展示未来 10 次
- 测试规则是否符合预期
- 用户创建提醒时即时反馈
```

------

## 2. 多规则合并算法

V2 可能允许一个提醒有多个 schedule。

例如：

```text
工作日 09:00~12:00 每 30 分钟
周末 10:00~11:00 每 15 分钟
每月 1 号 20:00 固定提醒
```

可以建模为：

```text
Reminder
 └─ schedules: Vec<ScheduleRule>
```

计算下一次：

```text
next_fire = min(
  schedule_1.next_fire_after(now),
  schedule_2.next_fire_after(now),
  schedule_3.next_fire_after(now)
)
```

------

## 3. 防刷屏算法

V2 要处理密集提醒、休眠恢复、勿扰结束后的堆积问题。

建议有全局节流：

```text
max_visible_lanes
max_queue_size
max_catchup_per_minute
merge_threshold
```

合并策略：

```text
如果 10 秒内有多条普通提醒：
  合并为一条摘要

如果高优先级提醒：
  单独显示
```

示例：

```text
你有 5 条提醒：喝水、拉伸、站立、复盘、邮件
```

------

## 4. 低资源动画策略

V2 仍要坚持低资源。

策略：

```text
- 无提醒时完全不渲染
- overlay 隐藏时释放部分图形资源，或保留轻量资源
- 动画帧率可配置
- 电池模式下降低帧率
- 字体布局缓存
- 同一提醒文本宽度缓存
```

------

# 十一、V1 到 V2 的演进顺序

不要在 V1 做太多 UI。推荐顺序是：

```text
1. V1 核心调度稳定
2. V1 飘字稳定
3. V1 CLI 可管理
4. V1 数据模型预留 display_json 和 schedule_json
5. V2 做设置界面
6. V2 做托盘
7. V2 做勿扰
8. V2 做多显示器
9. V2 做高级规则
10. V2 做诊断与历史
```

这样不会返工太多。

------

# 十二、关键风险与规避方案

## 风险 1：Windows Service 不能正常显示 UI

规避：

```text
不要让 Service 画 UI。
使用用户登录会话内的 Agent。
Service 最多作为 V2+ 可选组件。
```

------

## 风险 2：低资源目标被 UI 框架破坏

规避：

```text
Agent 不用 Electron / WebView。
设置界面独立进程，用完退出。
常驻部分只保留 Rust 原生后台逻辑。
```

------

## 风险 3：时间规则越来越复杂

规避：

```text
第一版只实现明确规则：
固定时间
每日
每周
时间窗口 + interval
日期范围
排除日期

高级 RRULE、节假日、自然语言解析放 V2 或以后。
```

------

## 风险 4：休眠恢复后提醒刷屏

规避：

```text
V1 默认 missed_policy = fire_once。
V2 增加 queue_limited 和摘要合并。
```

------

## 风险 5：多显示器和 DPI 适配复杂

规避：

```text
V1 只支持主屏。
V2 再支持多显示器。
每个显示器一个 overlay window。
```

------

## 风险 6：透明置顶窗口影响用户操作

规避：

```text
默认启用 click-through。
用户可关闭鼠标穿透。
提醒点击交互放到 V2。
```

------

# 十三、推荐开发优先级

## V1 优先级

```text
P0：
- 数据模型
- next_fire_after 算法
- MinHeap + 单 WaitableTimer
- 基础 overlay 飘字
- CLI 添加 / 查看 / 删除提醒

P1：
- 休眠恢复
- 系统时间变更重算
- 登录自动启动
- 多条提醒队列
- 日志

P2：
- 提醒历史
- 简单样式配置
- 安装 / 卸载脚本
```

## V2 优先级

```text
P0：
- 图形化设置界面
- 托盘菜单
- 规则预览
- 勿扰模式

P1：
- 多显示器
- Snooze 延后
- 点击提醒交互
- 历史与诊断界面

P2：
- 高级规则
- 外观主题
- 导入导出
- 可选 Service
```

------

# 十四、最终建议

第一版不要追求“大而全”。它应该像一个高质量系统组件：

```text
轻
准
稳
后台安静
到点可靠
提醒明显但不打扰操作
```

所以 V1 的核心开发顺序应该是：

```text
规则模型
→ 下一次触发算法
→ 低功耗调度器
→ 透明飘字窗口
→ CLI 管理
→ 登录自启动
→ 休眠恢复与异常处理
```

V2 再把它产品化：

```text
图形设置界面
→ 托盘
→ 多显示器
→ 勿扰
→ Snooze
→ 规则预览
→ 历史诊断
```

这条路线最符合你的原始要求：**Rust、Windows、高性能、低资源占用、后台运行、规定日期及时段提醒、支持自由时段与固定间隔、用屏幕滚动飘字提醒。**