# 下载恢复验证（2026-10-03）

本轮只修改下载恢复、进度和任务反馈，不是全产品 UI 重设计。遵循
`.agents/skills/course2md-design/SKILL.md`；沿用共享状态颜色、字号、控件和布局。
未安装或替换 `/Applications/course2md.app`，未改写个人任务、配置或异常媒体文件。
已安装 CLI 与桌面可执行文件的修改前后 SHA-256 完全相同。

## 自动化证据

最终源码执行：

```sh
cargo test --locked --features integration
cargo clippy --locked --all-targets --features integration -- -D warnings
cargo test --locked --manifest-path desktop/Cargo.toml
cargo build --locked
cargo build --locked --manifest-path desktop/Cargo.toml
```

- CLI：163 单测、26 集成测试通过；2 个需 Bilibili 网络的测试忽略。
- 桌面：236 测试通过；3 个需外部媒体工具或公共网络的预览测试忽略。
- Clippy 通过。GPUI 上游依赖仍有既有编译警告，不算作应用测试失败。
- `src/fetch.rs` 的离线回归运行真实 yt-dlp 和 localhost HLS/音频 fixture
  （`tests/fixtures/download_recovery.py`）：视频完成后音频 403，再提取并恢复时，
  视频片段只请求一次，合并后的视频仍为 12 帧，没有第二份视频。
- 同一回归读取实际 yt-dlp 进度模板，验证视频/音频合计、小数估算、缓存复用
  和重试不会重复计数。权限、私有视频和非 YouTube 下载不使用媒体 403 刷新策略。
- 单测覆盖未知总量、科学计数法、流切换速度重置，以及旧协议兼容。

日志：`/tmp/course2md-cli-download-final.log`、
`/tmp/course2md-cli-clippy-final.log`、`/tmp/course2md-desktop-download-final.log`。

## 原生运行证据

独立 debug bundle、配置、库和媒体均位于
`desktop/target/validation/download-recovery/`。仅显式合成来源
`https://www.bilibili.com/video/BV1UXTRANSFER` 使用受控进度样本；
普通 URL 仍调用真实 yt-dlp。字幕解析、ffmpeg、worker、保存和阅读器是真实实现。
该 UI fixture 不证明线上 YouTube 的 403 原因或必定恢复；真实下载行为由上述离线回归验证。

从隔离保存的已读取输入，使用原生按钮执行了：

1. 空任务页 → 工作台「开始转换」。
2. 视频阶段：显示合计和「总量为估算」，不伪装为精确总量。
3. 音频阶段：已完成的视频保留在合计内，没有重置为只显示音频大小。
4. 暂停：信息反馈而不是错误；保存进度。取消只作用于隔离任务。
5. 合成 HTTP 403：工作台与任务页直接显示原因、保留下载和重试提示；技术详情仍可用。
6. 「继续任务」恢复后完成；停留任务页不抢走导航。点击「阅读笔记」进入可读笔记和真实截图。
7. 同一来源「生成新版笔记」的失败不覆盖先前已完成版本。

截图通过 macOS `screencapture` 捕获实际 GPUI 窗口，位于上述目录的 `screenshots/`：

| 证据 | 条件 |
| --- | --- |
| `empty-native.png` | 空任务页 |
| `video-progress.png`、`audio-progress.png`、`paused.png` | Paper，100%，1140×820 |
| `failure-workbench.png`、`failure-task-final.png` | HTTP 403，默认尺寸；后者含保留的估算说明 |
| `failure-task-narrow.png` | Paper，100%，900×760 |
| `recovery.png`、`reader.png` | 恢复完成且可进入真实阅读器 |
| `failure-dark-200.png` | Nord，200%，1320×920 |
| `failure-dark-200-narrow.png` | Nord，200%，900×820；错误、阶段说明自然换行，恢复按钮仍可见 |

审查记录：

- 发现失败后任务只剩字节数量，估算标记与阶段丢失。已让保留的下载行显示
  原始阶段的中文说明，新增单测，并在最终原生截图确认。
- 403 摘要不能误标 AI 凭据或网页访问失败。界面与下载器共享媒体 403 分类，
  两类反例及暂停反馈均有单测。
- 主标题与卡片使用既有 pane 边界；下载行沿用图标、标签、状态、数据列。
  新提示是普通正文，不创建新容器、字体或状态配色。窄窗/200% 长文本能换行，
  未遮挡恢复操作。
- Peekaboo 的元素点击在本机可能兼用 AX 动作和坐标事件，造成恢复后又暂停；
  改用只发送一次的坐标点击，确认原生恢复成功。其早期 121×127 缩略图不作为验收证据。

范围限制：未遍历其余 8 个调色板、125/150% 字号、所有键盘/滚动/运动组合；
没有进行全产品或专项可访问性审计。自动化键盘输入受本机权限限制，故输入以隔离
保存的 fixture 提供，不能据此声称验证了真实粘贴操作。真实网络的限流、权限和
出口差异仍可能导致 403；新的有限恢复不是绕过服务器权限。

## 可运行预览

在本机重新打开已生成的隔离预览（不会使用个人库）：

```sh
open 'desktop/target/validation/download-recovery/course2md Validation download-recovery.app'
```

受控下载阶段由该根目录的 `download-phase.json` 决定：
`video` / `audio` 阻塞相应阶段，`error` 触发 403，`finish` 放行完成。
例如先写入 `{"phase":"finish"}`，再点击「继续任务」即可观察恢复。

重新准备一套独立预览时，先构建 debug 二进制，然后：

```sh
python3 desktop/scripts/ux_fixture.py prepare --root "$PWD/desktop/target/validation/download-recovery"
python3 desktop/scripts/ux_fixture.py bundle --root "$PWD/desktop/target/validation/download-recovery" --label download-recovery
```

预览不会迁移或清理旧版的 `.mp4.part*` 文件。生产修复将这些未校验缓存保留在原位，
后续下载使用 URL/画质隔离的新缓存目录；不要在仍运行旧 release 的任务中反复点击继续。
