# DotWallpaper macOS 验收清单（v0.1.2）

> 在真实 macOS 设备（Apple Silicon，macOS 26+）上验证当前版本功能与稳定性。
> 清单只覆盖 **已实现的能力**：GIF、动态 HEIC（solar/h24/appearance）、网页壁纸、
> Steam Workshop、每 Space 独立配置均 **不在本版范围内**，不要按"应支持"来验收。

## 版本边界（先对齐预期）

| 维度 | 本版范围 |
|------|----------|
| 系统 | macOS 26+，Apple Silicon（arm64）；无 Intel / Universal 包 |
| 静态图片 | JPG / JPEG / PNG / BMP / WebP / HEIC（静态） |
| 动态壁纸 | MP4 / MOV（AVFoundation 桌面播放层） |
| 明确不支持 | GIF 动画、动态 HEIC、m4v/其他容器、网页/Shader/粒子壁纸 |
| 签名 | ad-hoc（`codesign -s -`），未做 Developer ID 签名与公证 |
| 显示器 | 多屏独立壁纸；不支持每 Space 独立配置；不接管锁屏/登录屏 |
| 启动 | 无开机自启，需手动启动应用；关窗后菜单栏常驻维持播放 |

## 前置条件

- [ ] Apple Silicon Mac（M1/M2/M3/M4），macOS 26 或更高
- [ ] 至少一台外接显示器（多屏与热插拔用例需要）
- [ ] 测试素材：
  - [ ] 静态图片：JPG / PNG（含透明通道）/ BMP / WebP / 静态 HEIC
  - [ ] 视频：MP4、MOV（H.264 与 HEVC 各一）
  - [ ] 反例素材：GIF、动态 HEIC、m4v、损坏/不可解码的 mp4
  - [ ] 大目录样本：约 1000 张图片的文件夹
- [x] `npm run build --prefix ui`、`cargo test --target aarch64-apple-darwin` 全绿（本机 2026-09-22 实测：前端 `vue-tsc --noEmit` + vite build 成功、`cargo test` 16/16、`cargo clippy --all-targets` 零告警；验证人接手时需重跑一遍）

## 1. 基础功能

### 1.1 启动与界面
- [ ] 应用正常启动，无崩溃；主窗口深色三段式布局渲染正确
- [ ] 菜单栏图标出现，菜单含「打开 / 暂停全部 / 恢复全部 / 停止全部 / 退出」
- [ ] 顶栏：目录名（完整路径在 tooltip）、选择文件夹、导入、刷新、显示器下拉
- [ ] 顶栏按钮在窄窗口下为图标 + tooltip，不换行、不溢出

### 1.2 快照与显示器
- [ ] `get_app_snapshot` 返回库目录、默认显示方式/静音、显示器数组、状态数组
- [ ] `list_displays` 列出全部在线显示器，主显示器 `primary = true`
- [ ] 逻辑边界 `logicalBounds` 与缩放比与系统设置一致
- [ ] 内置屏 ID 固定为 `builtin`；有序列号的外接屏 ID 为 `disp-<vendor>-<serial>`
- [ ] 无 EDID 序列号的显示器退回 `cgdisplay-<id>` 临时 ID（`temporary = true`）：顶栏下拉标注「（临时）」、状态面板标注「临时 ID」，tooltip 说明拔插后需重新指定
      （ID 组装规则已拆成纯函数并有单测 `stable_display_id_rules`；注意对无效 CGDisplayID 直接查
      `CGDisplayIsBuiltin` 不可靠——本机实测会把 `0xFFFFFFFF` 当作主屏返回 `builtin`，故规则必须与查询解耦）
- [x] `updateSettings` 的 native bridge 参数名与 Swift dispatcher 一致
      （`snake_case` 参数会被转成 lowerCamelCase）。此前后端参数叫 `new_settings`（期望键 `newSettings`）、
      前端却发 `{ settings }`，命令必然以 `missing required key` 失败，界面表现为「保存设置失败」且
      显示方式/静音偏好从不落盘。已把参数改回 `settings`；因 `vite build` 不做类型检查，另加
      `settings_wire_contract_both_directions` 钉住双向字面 JSON，并用
      `invoke_arg_keys_match_command_parameter_names` 交叉校验全部 10 个命令的键名
      （从 `main.rs` 与 `api.ts` 双份源码解析；已做反向验证：把前端键临时改名后该测试确实失败）
- [x] 库目录不再经 `update_settings` 回写：旧实现每次保存都把前端的 `libraryDir` 写进设置，而快照未读回
      或读取失败时它是空串，此时点一下静音就把空目录落盘，下一次 `list_media` 报「无法读取壁纸目录 」。
      目录改为只能由 `pick_library_directory` 修改（该命令自己落盘 + 授权 asset scope），
      `FrontendSettings` 加 `deny_unknown_fields`，`settings_wire_contract_both_directions` 断言带目录的旧载荷必须解析失败；
      本机当前 `settings.json` 实测 `libraryDir` 仍为 `/Users/…/Pictures`（该缺陷是潜在路径，未在本机触发过）
- [ ] 真机确认：切一次「适应」并取消静音 → 无「保存设置失败」toast → 重启应用后仍是「适应」且未静音
      （`~/Library/Application Support/…/settings.json` 里 `defaultFitMode` / `defaultMuted` 已更新，
      且 **`libraryDir` 保持原值不变**——这是上面那条回归的直接验证）
- [ ] 真机确认：换一次壁纸库目录 → 顶栏目录名/tooltip 立即更新 → **不重启**直接点静音再重启，
      目录仍是新值（旧实现在这条链路上会把目录写坏）

## 2. 静态壁纸

- [ ] 点击壁纸卡片选中 → 「应用壁纸」后系统桌面立即更新
- [ ] 填充 / 适应两种显示方式效果正确
- [ ] 状态镜像与后端 `apply_wallpaper` 返回值一致（phase = `static`）
- [ ] 格式：JPG / PNG（透明）/ BMP / WebP / 静态 HEIC 均设置成功
- [ ] 反例：GIF、m4v、不支持扩展名被拒绝并给出可读提示
- [ ] 多屏：为不同显示器分别设置不同图片，互不影响
- [ ] 越界防护：`apply` / `delete` 作用于库目录之外的路径被拒绝

## 3. 动态壁纸（视频）

### 3.1 播放
- [ ] 选择 MP4/MOV 并应用：先出现「正在准备动态壁纸…」，就绪后才提示「已启动」
- [ ] 循环播放无可见接缝；默认静音；取消静音后有声
- [ ] 播放窗口位于桌面图标之下，不拦截鼠标事件（图标仍可选中/拖动）
- [ ] 在所有 Space 中显示；Mission Control / 调度中心下表现正确
- [ ] 暂停 / 恢复 / 停止均生效，且 UI 徽章与桌面实际表现一致

### 3.2 事务式切换与回滚（P0 重点）
- [ ] 快速连续切换 A → B → C：最终只播放 C，无叠窗、无黑屏、无残留窗口
      （代码保证：`watch_step` 只取走代数相同的候补，过期的 A/B watcher 直接静默退出，
      不会把 C 的候补误当自己的销毁；`fail_staged` 同样先校验代数再销毁）
- [ ] 切换过程中旧视频持续显示，直到新视频首帧就绪才替换
- [ ] 未勾选静音时：准备阶段（最长 ~15s）不出声，候补窗口上位那一刻才按音量偏好出声
- [ ] 选用不可解码的 mp4：旧视频继续播放，系统壁纸不变，前端提示失败并保留原壁纸
- [ ] 播放器准备超时（约 15s）：候补会话被销毁，旧画面保留，状态回到 `error`
- [ ] 视频准备中点击「停止」：watcher 作废，视频 **不会** 在停止后突然开播
- [ ] 视频准备中：「暂停」「恢复」为**禁用**状态（后端此阶段拒绝这两个动作），且按钮下方显示
      「动态壁纸准备中：可以立即停止，暂停 / 恢复要等首帧就绪。」；首帧到位后两按钮自动可用
- [ ] 显示器带壁纸时被拔出：右侧「显示器状态」出现一行「已断开显示器 · 未在线」（错误文案「显示器已断开」，
      卡片保留它占用的文件名），顶栏下拉框对该选择显示「该显示器已断开」而不是空白；
      此时「应用壁纸」禁用并给出说明，按钮位显示「解除占用」
- [ ] 接上条：点「解除占用」→ toast「已解除占用：该显示器的壁纸配置已清除，文件可以删除了」→
      该文件在网格里失去"已作为壁纸"圆点 → 删除成功（此前它会被"记住的壁纸配置"永久锁住）
- [ ] 拔屏 / 重连后**不按 ⌘R**：顶栏下拉框与右侧列表在 1~2s 内自动反映真实显示器
      （代码保证：`wallpaper-state` 事件里出现未知 ID 或 `error` 阶段时自动 `loadSnapshot()` 重取列表）
- [ ] 从视频切静态图片：先设置并读回确认新图片成功，之后才销毁视频会话；
      设置失败时视频继续播放，不出现"丢壁纸"
- [ ] 视频成功播放后，系统静态背景被同步为该视频首帧海报（退出应用后桌面仍是合理画面）

### 3.3 配置记录时机
- [ ] 视频停留在 Preparing 时不写入持久化分配；仅 Playing / Paused 后写入
- [ ] 应用失败的视频不会在下次启动时被反复重试
- [ ] 用户显式「停止」后，重启应用不再自动恢复该屏视频

## 4. 状态同步（前后端事件）

- [ ] `wallpaper-state` 事件驱动 UI：Preparing→Playing 无需手动刷新即更新徽章
- [ ] 播放失败时前端弹出错误提示，显示器状态卡片显示错误文案
- [ ] 拔掉显示器 → 前端显示器卡片显示"已断开"，无需刷新
- [ ] 重新插入 → 显示器下拉与卡片自动恢复（快照自动补刷）
- [ ] 休眠唤醒后前端状态与桌面实际一致
- [ ] **镜像**：接第二块屏并在系统设置里开「镜像」→ 副屏那一行的下拉里应出现「镜像」徽章，镜像源那块仍无徽章
      （`CGDisplayIsInMirrorSet` 本机单屏无从触发，目前只做到「符号能链接、枚举不崩、单屏下仍为 false」）
- [ ] `thumbnail-ready` 事件回填：后台生成完成的缩略图/海报逐张出现在网格里

## 5. 媒体库体验

- [ ] 首次加载：骨架屏占位；超过 ~1.2s 显示「正在读取壁纸目录…」
- [ ] 读取失败：把壁纸目录改名/移走后再刷新 → 明确错误态 + 「重试」/「选择其他文件夹」，不与空目录混淆（`list_media` 此时返回 Err；未选目录或空目录仍返回空列表）
- [ ] 空目录：引导文案 + 当前目录路径，而不是"正在加载"
- [ ] 搜索框按文件名过滤；图片 / 视频类型筛选可用；无匹配结果时有清除筛选入口
- [ ] 排序下拉（名称 A→Z / Z→A、最近修改 / 最早修改）对**当前筛选结果**生效，切换后回到首批；
      改动时间取不到的文件（`mtime = 0`）在两种时间序里都按名称排在末尾，不会冒充"最早修改"
      （`media_default_order_is_case_insensitive_and_mtime_is_reported` 已覆盖后端默认序与 `mtime` 字段序列化）
- [ ] 约 1000 张图片目录：首屏可交互 ≤ 2s，滚动到底分批追加而不卡顿
      （本轮把「问尺寸」从整幅解码改成只读 ImageIO 属性字典，省掉的正是每张一次全解码——空缓存下重扫最能体现）
- [x] 符号链接不污染列表：`media::scan` 改用 `file_type()`（lstat，不跟随）判定，软链既不入列也不递归
      （跟随的旧写法会让自我引用软链把栈撑到无限、库外文件被列成看得见点不动的死条目；
      `scan_ignores_symlinks_instead_of_recursing_or_listing_them` 用「指向自身的目录软链 + 指向库外 jpg 的文件软链」
      钉住只返回那一个真实文件——回归时表现为该测试挂死而非断言失败）
- [ ] 拖入导入：进入显示遮罩、松开后复制入库并自动选中新导入项
- [ ] 「导入」按钮走原生 NSOpenPanel，可多选
- [ ] 视频缩略图取首帧海报（不再是空白占位）；海报缓存在重启后复用
- [ ] **首扫几百个视频的库时窗口仍可点击/滚动**：海报只能在主线程抽取，代码已把投递排成每 120ms 一个、
      落后的 worker 睡在自己线程上（`thumbs.rs` `POSTER_SPACING`）；空缓存下重扫时点一下侧栏/滚动网格验证不卡死。
      排期只影响视频项首屏就绪的数量（其余靠 `thumbnail-ready` 逐张回填），图片路径不经过主线程
- [ ] 缓存名含源文件内容指纹（mtime+大小）：同名覆盖替换图片/视频后，网格与壁纸显示的都是新内容的缩略图
- [ ] 删除当前正被某显示器使用的文件被拒绝，提示先停止/切换
- [ ] 「停止」视频后**不重启**直接删该视频：应被拒绝，原因写明「桌面留的是它的首帧海报，删文件会连海报一起清掉」；
      界面上这张卡片仍带使用中角标，两边说法必须一致（同一屏改贴别的壁纸后即可删除）
- [x] 删除保护 fail-closed（代码保证，真机难以复现）：`is_path_in_use` 只在「应用尚未初始化」时返回未使用，
      主线程联系不上时一律按"使用中"拒绝删除——不会因调度异常放走正在播放的文件；
      `is_path_displayed`（状态镜像那条）读不到镜像时同样返回"在用"
- [ ] 删除未被使用的文件成功，同时清理其缩略图与海报缓存（含内容变化后遗留的旧缓存名）

## 6. 生命周期

- [ ] 关闭管理窗口后播放继续；点 Dock 图标（`RunEvent::Reopen`）重新唤回管理窗口
- [ ] 菜单栏「退出」：播放窗口全部销毁、无残留进程、候补会话同样被清理
- [x] 单实例护栏：第二个实例启动即退出（本机 2026-09-22 实测：已有实例在跑时再启动一次，
      `pgrep -x dotwallpaper` 数量不变，stderr 打印「[DotWallpaper] 已有实例在运行，本次启动直接退出」，
      `~/Library/Application Support/com.dot.wallpaper/single-instance.lock` 存在）
- [ ] 真机确认：`open WallpaperEngine.app` 连点两次 Dock 图标只有一份进程/一份菜单栏图标；
      结束后（含 `kill -9`）再启动能正常打开（flock 由内核随 fd 释放，不需要清理锁文件）
- [x] 退出清理不再只挂在托盘「退出」上：此前 `teardown_all_sync` 仅在托盘菜单项里调用，
      Cmd+Q、Dock 右键退出、以及任何 `app.exit()` 路径都会绕过它，桌面级播放窗口可能残留。
      现改挂在 `RunEvent::Exit` 上（源码确认：该事件在退出线程主线程、`cleanup_before_exit()` 之前
      回调，`on_main` 因已在主线程而内联执行 → 仍是同步清理），托盘项只留 `app.exit(0)`，
      所有优雅退出路径收敛到一处；`teardown_all_sync` 幂等，重复进入空转
- [ ] 真机确认上面这条：有视频在播时按 Cmd+Q（而不是托盘退出）→ 进程消失且桌面不留播放窗
- [x] 退出时状态镜像被清空（`teardown_all_sync` 末尾 `states().clear()`）：退出流程中任何后续快照/事件都不会再报 `playing`
- [ ] 系统休眠 → 唤醒：自动恢复播放；分辨率变化后播放窗口重定位正确
- [ ] 稳态（既不休眠也不变化）下监视线程不重复重排窗口/调用 play，仅在「休眠→唤醒」或几何变化那一轮触发
- [ ] 拔掉显示器：该屏播放停止，另一屏不受影响
- [ ] 重新插入显示器：按持久化配置自动恢复
- [ ] 恢复失败（如视频被外部删除）：状态经事件标为错误/静态且只报一次，监视线程不会每 2 秒对同一份坏配置空转重试；改应用可用文件后锁自动解除
- [x] 「解码失败 / 15s 准备超时」也纳入同一把恢复失败锁（`fail_staged` 销毁本代候补时按「该屏 + 该文件」锁定）：
      重插一块配置了坏视频的显示器不会再陷入 应用→准备失败→重试 的循环，也不会反复弹错误 toast；
      `apply` 成功路径统一 `clear_failure`，因此换内容修好同名文件后重连仍能自动恢复（代码层修复，真机需复插验证）
- [ ] 重启应用：按上次配置恢复各屏壁纸，缺失媒体只标记错误、不阻塞其他显示器
- [x] 配置版本迁移真机验证（本机 2026-09-22）：v1 `settings.json`（`disp-4a8b-906d7-1010101`，内嵌旧 CGDisplayID）
      在启动载入后被改写为 `disp-4a8b-1010101` 并以 `version: 2` 落盘，`displayId` 与键同步，应用无报错、进程稳定；
      该屏当前不在线，故只发布「显示器未连接，重连后将自动恢复」而不重复套用（单元测 `legacy_display_ids_migrate_on_load` 另覆盖幂等性）
- [ ] 在某屏「停止」后重启：该屏不再自动恢复该动态壁纸（持久化分配已清除），且该文件解除删除保护可以删掉
      ——状态镜像是内存态，重启后为空，所以「停止但不重启」时它仍受上面那条海报保护
- [ ] ⌘R 刷新列表、⌘S 应用当前选中项

## 7. 性能与资源

- [ ] 两屏 1080p30 视频连续 30 分钟：UI 不卡顿，无掉帧堆积
- [ ] 稳定期 RSS 不持续增长（重点观察反复切换/停止后的内存曲线）
- [ ] 暂停后 CPU 显著下降；停止后播放线程与窗口资源释放
- [ ] 首屏与切换响应：切换壁纸 ≤ 500ms 给出反馈，视频真正启动 ≤ 15s
- [x] 常驻 30 分钟空载内存基线（本机 2026-09-22 10:59–11:29，`ps` 每 30s 采样，60 个点）：
      RSS 108.4–108.8 MB（110,992–111,376 KB），极差 384 KB、无上升趋势（末值即最小值），
      线程数稳定 15（瞬时峰值 19），实例数恒为 1。
      **注意：这是「无播放会话」的空载值**——本机唯一显示器为临时 ID，配置里的 `disp-4a8b-1010101`
      对不上任何在线屏，所以整段没有视频在解码。带 1080p30 视频播放的曲线仍须真机补测

## 8. 安全与权限

- [ ] 无需辅助功能权限
- [ ] 无需录屏权限
- [ ] 无需完全磁盘访问权限（文件访问经系统对话框 / 库目录边界）
- [ ] 不修改系统壁纸数据库之外的内容，不重启 Dock
- [x] WebView 本地资源加载：Vite 使用相对 base，构建后 JS/CSS 内联进 `ui/dist/index.html`，发布运行由原生 WKWebView 加载内联资源。
      壁纸库目录改由运行时 `ensure_asset_scope`（启动 setup + 每次选目录）按当前配置逐目录授权。
      此前静态 scope 里还写着 `$HOME/Pictures/**`、`$PICTURE/**` 和两个系统桌面图片目录，等于让
      WebView 无条件可读整个 ~/Pictures，而库目录其实已经由运行时授权覆盖，故删除
- [ ] 真机确认收窄后仍能出图：图片卡片缩略图、预览面板原图均正常加载，无 `convertFileSrc` 403/空白
- [x] 构建配置统一由 Xcode 工程与构建脚本维护；迁移期间重复的旧桌面打包配置已清理。

## 9. 已知限制（按设计，不算缺陷）

- [ ] 无 EDID 序列号的显示器只能拿到临时 ID（`cgdisplay-<id>`）：重新插拔或重启后该 ID 会变，
      之前给它配置的壁纸不会自动恢复，必须在界面（已标注「临时」）上重新指定一次——设计如此，非缺陷。
      本机实测：唯一在线显示器 vendor=`0x756e6b6e`（ASCII "unkn"）、serial=`0` → `temporary = true`
- [ ] 动态壁纸依赖应用进程常驻；退出后仅保留视频海报作为桌面背景
- [ ] 不支持每个 Space 独立配置壁纸
- [ ] 不支持锁屏 / 登录屏接管
- [ ] 无开机自启，重启系统后需手动启动
- [ ] ad-hoc 签名：分发给他人需先补 Developer ID 签名 + 公证（见 build-guide）
- [ ] 仅 arm64，Intel Mac 与 Universal 包尚未纳入

## 10. 发布质量（正式版前必须补齐）

- [x] `npm run release:mac` 全链路可用：构建 → `strip -x` → 重签（默认 ad-hoc）→ `codesign --verify --strict` 自检 → `hdiutil` UDZO DMG（本机 2026-09-22 多轮实测：strip 后二进制 7,589,008~7,589,264 B、.app 7.4 MB、DMG ≈1.92 MB；DMG 每轮压缩有 1~4 KB 浮动（同一份代码 1,914,193~1,916,299 B，海报排期 + 设置原子落盘 + 尺寸读取改造之后 1,917,822~1,919,765 B），仅有 hdiutil 参数弃用提示，不影响产物）
- [x] `settings.json` 落盘改为「同目录临时文件 + rename」：`fs::write` 会先截断再写，进程在写一半时被杀（`panic = "abort"`、强杀、断电）会把配置撕成半截，解析失败即整份回退默认——所有显示器的壁纸再也恢复不了。顺带修掉一个目录错误：`init()` 里 `app_config_dir()` 失败时兜底曾是**壁纸库目录**，会把 `settings.json` 写进 `~/Pictures`（还会被当成媒体扫进列表），现兜底为 `~/Library/Application Support/<identifier>`。`settings_roundtrip_corruption_and_version` 钉住 `.tmp` 不残留与落盘内容可读
- [x] `set_static_wallpaper` 的 options 字典不再泄漏（`msg_send![new]` 返回 +1 引用，改为接成 `Retained<NSMutableDictionary>` 由所有权释放）；`image_size` 改为只读 ImageIO 属性字典，省掉「问一次尺寸就整幅解码一次」
- [x] `DisplayInfo.mirrored` 不再恒为 `false`：接上 `CGDisplayIsInMirrorSet`，前端 TopBar 的「镜像」徽章从死代码变成有数据来源（真机镜像场景仍待人工验收）
- [x] 签名自检已进流水线（每次 `release:mac` 都会跑）；顺带去掉了 `codesign --deep`——Apple 已弃用该选项，且本包只有一个主二进制、无嵌套代码，内→外分别签即可
- [x] `codesign --verify --strict` 与 `hdiutil verify` 对最终代码产物再次通过
- [x] `codesign --verify --strict` 通过、`hdiutil verify` 通过；`codesign -dvv` 为 `Signature=adhoc` / `TeamIdentifier=not set`
- [x] 打包脚本已收敛（原「待定」已决定）：根构建已切换为原生 Xcode：`npm run build:mac:app` 生成 `WallpaperEngine.app`，`npm run release:mac` 通过 `diskutil image create from` 生成 DMG；由 Xcode 原生构建与 macOS 磁盘映像工具完成。
- [x] 签名命令与参数已验证可用（本机临时副本试签）：`--options runtime --timestamp` 生效，`codesign -dvvv` = `flags=0x10000(runtime)` + `Runtime Version=27.0.0` + 安全时间戳，`--verify --strict` 通过；`spctl` 仍 rejected 且 `origin=Apple Development`
- [x] Developer ID + 公证路径已固化进脚本（`DW_SIGN_IDENTITY` / `DW_NOTARY_PROFILE`）：签 → 自检 → **再**出 DMG → 公证 DMG → staple DMG 与 .app → `spctl --assess`。顺序是关键——先出盘再签名的 DMG 等于没签。fail-closed 已负向实测：给一个不存在的身份，`codesign` 报 `no identity found`，脚本 exit 1 且不产出 DMG，不会静默退回 ad-hoc
- [ ] Developer ID Application 签名（含 hardened runtime）——命令已在脚本里，本机 keychain 只有 `Apple Development` 一张证书，需先申请 Developer ID 证书后跑一次正向产物
- [ ] `notarytool` 公证 + `stapler staple`（本机 `notarytool 1.1.3` 可用，缺 Developer ID 与公证凭据）
- [x] 分发链路实跑：DMG 校验 → 挂载 → 盘内 `WallpaperEngine.app` 通过 `codesign --verify --strict` → 可复制到 `/Applications` 并启动；当前 ad-hoc 包只作本机/内部测试。
- [ ] `spctl --assess` 通过，Gatekeeper 首启动不拦截（当前 ad-hoc 包在 **assessments enabled** 下实测 `spctl -a -t execute` = rejected）
      ⚠ 口径说明：`sudo spctl --global-disable` 之后该命令必然返回 `accepted / override=security disabled`
      （本机 2026-09-22 实测），这是全局放行的结果而**不是**发布通过信号；只有在 `--global-enable`
      状态下测出的 accepted 才算数。上面的安装链路是在 Gatekeeper 关闭期间做的，所以它验证的是
      "包能装能跑"，不验证"别人下载不会被拦"——后者仍然只能靠 Developer ID + 公证
- [ ] 首启动权限/目录选择流程实测无卡死
- [ ] Intel / Universal 决策明确（做或写清不支持）
- [ ] 版本号、更新与升级策略确定

## 接手须知（2026-09-23）

**当前代码验证状态（2026-09-23）**：前端构建通过，Rust `cargo test` 16/16 通过，Xcode 27 构建 `.app` 成功（arm64，最低系统版本 26.0），`git diff --check` 通过。本轮未重新构建 DMG，也未进行 GUI/视频播放验收。

**只有你能推进的三件事**

1. 恢复 Gatekeeper（本轮为验证安装链路临时关闭）：`sudo spctl --global-enable`。
   之后需重测 `spctl --assess --type execute -vv /Applications/WallpaperEngine.app`，并把真实结果写回 §10——ad-hoc 包预期仍是 `rejected`，这条在拿到 Developer ID 之前不可能变绿。
2. 申请 Developer ID Application 证书 + 公证凭据，再跑正向链路：
   `DW_SIGN_IDENTITY="Developer ID Application: … (TEAMID)" DW_NOTARY_PROFILE=<profile> npm run release:mac`
   （脚本顺序是「签 → 自检 → 出 DMG → 公证 → staple → spctl」）。§10 剩下两条未勾项必须靠这一步的真实产物才能勾。
3. 必须**真的点开管理窗口**才能验的项：§5 全部（缩略图渲染、约 1000 张首屏 ≤2s、排序下拉、拖入导入）、§3 / §3.2 热插拔四步、镜像徽章、§6 休眠唤醒与「关窗后继续播放」、§9 的 30 分钟内存。
   注意：重启应用本身**不会**扫库、也不会生成缩略图（列表只在窗口挂载时拉），所以这类验证不能靠 `open` + 等待完成。

## 验证结果

| 项目 | 结果 | 备注 |
|------|------|------|
| 基础功能 | ☐ 通过 ☐ 失败 | |
| 静态壁纸 | ☐ 通过 ☐ 失败 | |
| 视频壁纸（含事务/回滚） | ☐ 通过 ☐ 失败 | |
| 状态事件同步 | ☐ 通过 ☐ 失败 | |
| 媒体库体验 | ☐ 通过 ☐ 失败 | |
| 多显示器 / 热插拔 | ☐ 通过 ☐ 失败 | |
| 生命周期 | ☐ 通过 ☐ 失败 | |
| 性能 | ☐ 通过 ☐ 失败 | |
| 安全 / 权限 | ☐ 通过 ☐ 失败 | |
| 发布质量 | ☐ 通过 ☐ 失败 | |

**验证人：** ___________
**验证日期：** ___________
**macOS 版本：** ___________
**设备型号：** ___________

## 原生启动故障复核（2026-09-24）

- [x] 修复空白窗口：Vite 内联 bundle 从 `<head>` 移到 `#app` 后；构建阶段加入回归测试，缺少根节点会失败，不再默默发出空白包。
- [x] 修复 `MainThreadMarker` panic：后台调用 AppKit 的工作异步投递到真正的 main queue，然后等待结果；旧同步派发可能在调用线程执行。
- [x] 媒体扫描转后台；未完成首次引导时不抢先扫描旧目录，扫描超时 15 秒向界面报错，可选新目录重试。本机旧目录读取曾在 `opendir` 阻塞数分钟，并非已经完成扫描的证据。
- [x] 本机真实打开 Xcode 27 原生 `.app`，确认管理界面、首次引导和显示器选择器可见；UI 构建（含 2 项回归测试）、Rust 16/16、Xcode 构建、签名自检和 DMG 校验通过。
- [ ] 用原生文件夹选择器完成目录授权，并确认原目录缩略图加载。
- [ ] 应用实际视频，验收播放、暂停/恢复、关窗后菜单栏控制和退出清理。

唯一当前产物：`build/xcode-derived/Build/Products/Release/WallpaperEngine.app` 与 `build/WallpaperEngine_0.1.2_arm64.dmg`。旧的 `build/WallpaperEngine.app` 和 `aarch64.dmg` 已移入忽略的 `build/legacy-artifacts/`，不可用作验收。上方 2026-09-23 验证状态是历史记录，以本节最新状态为准。

## 2026-09-24 视频准备与界面复核

- [x] 读取当日上午 10:31–10:46 的 `.ips`：旧运行包在 `on_main` 的 `MainThreadMarker` 断言中崩溃。当前 `.app` 曾是前一天的旧产物；重建后管理窗口可打开，没有新崩溃报告。
- [x] 发现首次引导/设置 CSS 没有从 Vue 入口导入；修复构建入口、低高度滚动布局，真实管理窗口的设置弹层已打开并显示目录、权限说明、默认播放和配置路径。
- [x] 只读 AVFoundation 探针针对当前视频：looper/player/currentItem 状态为 Ready，template 状态为 Unknown；说明旧模板检查会误报超时。应用改为检查实际项和三种失败状态。
- [ ] **应用内实际应用视频**，确认桌面图层循环、状态事件、暂停/恢复、退出后的海报；此次未更改用户现有桌面分配，不把只读探针算作此项通过。
- [x] 本机 `sample` 曾显示 `dw_list_media` 阻塞于系统 `opendir`，界面 15 秒后提示超时；通过设置中的原生选择器**重新选回同一个目录**后，界面加载 90 项媒体并更新列表（未更换目录或删除文件）。
- [ ] 在首次启动的独立用户配置中，实测窄窗口及低窗口高度下引导选择/跳过，并确认保存设置。

复核命令：`npm --prefix ui run build`（含 2 项内联回归测试）、`cargo test --manifest-path rust-core/Cargo.toml`（16/16）、`cargo clippy --manifest-path rust-core/Cargo.toml --all-targets -- -D warnings`、`npm run build:mac:app` 均通过。DMG 在本轮没有重新制作，不应当作当前版本。

## 2026-09-24 打开失败恢复及实际验证（上一轮状态）

- [x] 退出测试进程后，从备份恢复 `settings.json`；重启后首次引导仍已完成，原媒体库和三项显示器分配语义不变（仅 JSON 键顺序变化），未清除用户数据。
- [x] `build:mac:app` 原先只让 Xcode 生成 linker-signed 可执行文件，bundle 的 `codesign --verify --deep --strict` 失败；构建脚本现在为整个 bundle 做 ad-hoc 签名及严格校验。
- [x] 重新运行 `npm run release:mac`；新 DMG 通过 `hdiutil verify`，挂载后 `.app` 通过严格签名验证；从挂载 DMG 启动成功，管理窗口显示原媒体库 90 项，且没有新增 `.ips`。退出 DMG 实例后重开当前构建目录的 `.app` 成功。
- [x] 上轮交互实测 `2k_pro_60059.mp4` 应用后状态为「播放中」，暂停为「已暂停」，恢复为「播放中」；随后已恢复原静态壁纸。此处仅确认界面状态与命令行为，不算桌面视频画面、无缝循环与关窗菜单栏控制验收。
- [x] 上轮临时启动首次引导时，约 900×512 窗口中的标题、步骤与按钮可见。
- [ ] 真正缩至 800×480 验证滚动与按钮可达；视频桌面画面/循环、长时间播放和关窗后菜单栏控制仍待验。
- [ ] Gatekeeper 开启状态下用 Developer ID + 公证版本完成外部分发验收：当前 `spctl --assess` 仅显示 `accepted / override=security disabled`，不能视为发布通过。

注意：上方 2026-09-23 与 2026-09-24 早期条目保留历史测试范围；“未重新制作 DMG/未应用视频”的说明不适用于本节之后的新产物。

## 2026-09-24 再次打开失败：主线程崩溃修复与引导门控

- [x] 检查 10:31–10:46 的四份系统 `.ips`：均为 `SIGABRT`，崩溃栈落在 `engine::spawn_monitor → runtime::on_main → MainThreadMarker::new().expect`。即使 GCD 主队列执行回调，也没有保证此处被识别为真实主线程；不能把先前「主队列」修复算作此问题已关闭。
- [x] 主线程任务改投递到 CoreFoundation 的 **主线程 RunLoop**，同步路径返回显式错误，异步路径记录异常，不再让 `expect` 从 Rust FFI 边界中止整个应用。首次引导未完成时，启动恢复与监视恢复不读取旧媒体；引导完成后仅排队一次恢复，避免 300ms 启动窗口的双重恢复。
- [x] 低高度引导弹层的内容可滚动、底部操作按钮保持可见；阻止重复点击；设置保存失败时不关闭引导。生产前端资源配模拟 bridge 的 Chrome 800×480 截图仅验证网页布局，**不等于原生窗口可见性**。
- [x] Rust 16 项单测、Clippy `-D warnings`、发布 profile Rust 编译通过；当前用户 `settings.json` 与测试前备份 SHA-256 相同，未把 `onboardingCompleted` 留在 false。
- [ ] 当前 Mac 锁定且「访问文稿」弹窗尚未由用户处理；新包仍需在解锁后实际启动、观察 3 秒以上确认监视线程不再崩溃，并走完目录选择、视频桌面画面、循环与菜单栏操作。**构建/签名通过不能替代此项**。
- [x] 新 `.app` 与 DMG 已用 11:42 的前端资源（前端源码时间戳均早于资源）重建；Xcode 27 构建、ad-hoc 严格签名、arm64/最低 macOS 26.0、`hdiutil verify` 通过，**但这不是解锁后的打开验收**。
- [ ] 当前锁定状态下 Vite/esbuild 两次卡在系统 `__open`，本次原生构建仅临时复用了既有 `ui/dist`；解锁后仍需正常执行完整 `npm --prefix ui run build` 与 `npm run release:mac` 再验。

保留旧的历史验收条目用于追溯，以本节的未完成项为当前判断依据。公开分发仍需 Developer ID 签名、公证及 Gatekeeper 开启状态验证。

## 2026-09-24 目录选择持久化与保存错误（本轮待 GUI 复核）

- [x] 发现原生 `pickLibraryDirectory` 原先仅回传路径，未更新 `settings.json`；前端误认为目录已持久化，重启后仍用旧库，视频文件可能因不在旧库范围内而被拒绝。现通过 Rust 设置层先规范化、验证目录并原子写盘，**写盘成功才回传路径**；取消选择不改变配置。目录选择与设置保存失败均向界面返回具体错误，写盘失败恢复内存中的旧设置。
- [x] 新增目录选择成功/非目录失败后的内存和磁盘一致性、写盘失败回滚测试；Rust 单测 18 项与 Clippy `-D warnings` 通过。首次引导选目录后不等待媒体扫描才关闭弹层，扫描异步更新列表及错误提示。
- [x] 生产前端已在临时目录完成 TypeScript、Vite 与内联回归测试并复制到 `ui/dist`；原项目位于受系统弹窗影响的文稿目录时，直接执行 Vite 曾卡在系统 `__open`。发布构建临时复用**这次已完整构建**的 dist，不代表常规入口下的前端构建已通过。
- [x] 原生 Xcode 27 + Rust 发布构建与 DMG 制作完成；`codesign --verify --deep --strict`、arm64/最低 macOS 26.0、`hdiutil verify` 见本轮检查记录。产物为本机 ad-hoc 签名，不是已公证的分发包。
- [ ] Mac 当前仍锁定，文件访问弹窗需用户亲自处理。**新产物尚未实际打开验收**：解锁后使用本轮 `.app`，观察超过 3 秒确认监视线程不崩溃；选择新目录、重启确认路径保持，扫描并应用 MP4/MOV，检查桌面真实画面/循环、暂停恢复、关窗后菜单栏控制与 800×480 引导。遇到错误保留弹窗原文及新增 `.ips`，不得以构建成功或界面“播放中”替代实际视频播放。
- [ ] 解锁后还需在项目目录正常执行 `npm --prefix ui run build` 与 `npm run release:mac`，不能将临时构建 shim 当成正式构建配置。

## 2026-09-24 视频错误路径防中止加固（本轮）

- [x] 修复视频会话创建时对 `AVLayerVideoGravityResizeAspectFill/Aspect` 的 `expect`：可选常量缺失现在返回“视频填充模式不可用/视频适应模式不可用”，不会因 Rust panic 直接 Abort trap。
- [x] 修复停止动态壁纸时对不存在会话的 `map.remove(...).expect(...)`：现在返回包含显示器 ID 的可读错误，并保留其他显示器状态。
- [x] `cargo fmt --manifest-path rust-core/Cargo.toml`、`cargo test --manifest-path rust-core/Cargo.toml`（18 项）与 `cargo clippy --manifest-path rust-core/Cargo.toml --all-targets -- -D warnings` 通过。
- [ ] 仍未完成解锁后的 GUI 复测：需使用本轮重建的 `.app` 打开超过 3 秒，确认不再新增 `.ips`，再应用 MP4/MOV 验证桌面画面、循环、暂停/恢复、停止与菜单栏控制。不能用构建和单测替代这项。

## 2026-09-24 原生「设置」窗口补齐

- [x] 修复 macOS 应用菜单 `⌘,` 原本打开空白 `Settings { EmptyView() }`：现提供 SwiftUI 设置窗，可查看/重选媒体目录、阅读文件访问说明，并在 Finder 中定位配置与缓存。目录选择复用同一 Rust 持久化/错误返回路径，不另造第二份配置。
- [x] 在原生设置里更换目录后，向主 WKWebView 发设置变更事件；已有管理窗口重载快照与媒体列表。`vue-tsc`、Vite 生产构建、2 项内联测试在临时目录通过；Xcode 27 Release 构建、ad-hoc 严格签名、arm64/最低 macOS 26.0 和 DMG 校验通过。本次项目目录内的常规前端构建仍受锁屏/文件访问弹窗影响，Xcode 阶段复用已验证的临时构建资源。
- [ ] **GUI 尚未验收**：Mac 仍锁定。解锁后实际打开原生设置检查目录选择、Finder 按钮、主窗口同步与 800×480 引导；再应用 MP4/MOV，核实桌面画面、循环和错误提示。当前构建与签名通过不等于这些交互已成功。
