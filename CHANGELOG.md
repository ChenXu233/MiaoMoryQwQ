# 更新日志

由 CI 依据 conventional commits 自动生成，每次发布全量重写，请勿手改。

## 未发布

### 🐛 修复

- **region**：09-12 走查修复——区域检索三项+删除工作区事务化/TOCTOU+菜单失焦
## v0.1.1-alpha.1（2026-09-12）

### ✅ 测试

- **embed-bench**：对齐流水化 embed_worker——解码线程 8 张组经通道交推理

### ✨ 新增

- **p4**：口袋式数据布局——默认随安装目录，设置可迁移 (#4)
- **p5a**：来源文件夹（工作区）一等实体 + 源离线降级 + 占用分析 (#6)
- **p5d**：前端按原型 v6~v9 重写——壳/页面/composable 分层（UI 对齐落地） (#8)
- **p5b**：缩略图管线重做 + 导入内存治理（预算自适应/预取/丢全尺寸图） (#9)
- **p5c**：可插拔索引架构——Indexer trait / 多索引并存 / 多流置信度融合（ADR-0013）
- **p5b3**：HEIC 内嵌缩略图快路径（decode-spike §1，无需 ADR）
- **p6**：Watcher 自动同步+索引状态可见+侧栏/导入卡走查修复
- **inference**：推理后端运行时可配置——load-dynamic + EP 降级 + 运行时/模型导入命令
- **settings**：设置页二级分类导航 + 推理加速卡 + 模型本地导入
- **search**：结果透出语义相似度（余弦 0~1，前端显示百分比）
- **ui**：自绘窗口/右键/确认弹窗全量替换原生 + UI 设计语义修正
- **region**：ADR-0015 切片 A/B 落地——区域级语义索引管线与检索区域流
- **folders**：删除导入的文件夹（工作区）——级联记录/向量/区域/缩略图，原文件不动
- **ci**：发布 CI 全自动——打 tag 即发版（版本注入/git-cliff 发布说明/finalize 回写与更新器自管 latest.json）

### 🐛 修复

- **model**：模型下载时机前移至启动自动，文案纠正为导入与搜索共用 (#5)
- **p5**：实机走查修复六项——启动自动下载回归、模型加载清单文件名、库刷新事件等
- **p5**：走查第二轮三修——HEIC 索引源违规、删除后库刷新、模型准备中搜索状态
- **security**：安全加固三项——CSP、解码炸弹防护、索引注册校验
- **perf**：导入 IO 走查四修——嵌入错峰、hash 后去重预检、解码线程留余量、分段计时探针
- **mm-pipeline**：走查修复——HEIC 快路径尺寸写库、线程数下溢、解码防护补全
- **mm-store**：Register_index 建表与注册同事务
- **miaomory-app**：嵌入队列毒丸断路、KNN 单索引故障降级
- **miaomory-app**：UseLibrary 工作区切换失联与重载竞争
- **ui**：视觉走查——首页结果面板溢出窗口右缘 + 模型装配成功日志留痕
- **inference**：CUDA EP 加载预检（防 ort 段错误）+ 运行时清单回填 + 打包脚本
- **inference**：走查二轮修复——下载失败终态事件、配置防覆盖写、导入并发与主线程、watcher 重试
- **bundle**：Resources 平台化——tauri.windows.conf.json 承载 runtime/dml,修复 mac/linux 打包 GlobPathNotFound
- **region**：SAM ONNX 改文件路径加载(external data 无法从内存解析,静默失败根因)+ worker 分割器缓存 + asset protocol scope 重启重放(修 offline 一票否决根因)+ 诊断日志
- **folder**：重启后文件夹误报 offline 的死循环——scope 重放不按状态过滤 + report_original_missing 后端核实
- **region**：Decoder 16 点批对齐(GRID 4×4)+ image_embeddings rank4
- **region**：GRID 对齐 16 点批 decoder——区域提取全线打通
- **ui**：标题栏引入后的四处错位走查修复（playwright+CDP 截图走查）
- **menu**：右键菜单动作从不执行——capture once 全局关闭器先卸载菜单 DOM
- **ci**：推送前质量门预检——fmt 统一 + clippy 七处（区域切片首次过门）
- **bundle**：BeforeBuild/beforeDevCommand 平台化——prepare:runtime 移入 windows 覆盖层
- **ci**：Prepare-bundle-runtime 官方 zip 布局兼容——dll 递归定位
- **ci**：Chunks_exact→as_chunks——clippy 1.98 新 lint（工具链对齐 CI stable 后本地复检）
- **ci**：Windows 腿 DISM 启用 VBSCRIPT——WiX light.exe 的 MSI ICE 校验依赖
- **ci**：Inference ep 选定三变量补 cfg_attr(not(windows), allow(unused_mut))
- **ci**：根治 windows MSI 间歇性失败——build.rs 移除 ort-sys 侧车 DirectML.dll
- **ci**：Finalize 版本回写兼容 main 分支保护——被拒落 release-writeback 分支

### 📝 文档

- **agent**：开发流程改为默认直连 main——分支+PR 仅在所有者点名时使用
- **adr**：ADR-0013 可插拔索引架构（Proposed，所有者指令开工切片 C 为执行授权）
- **spec**：0007 合并规格——信息架构/来源离线/缩略图/可插拔索引/占用（P5 落实文档，行为+体验一体）
- **spec-0008/adr-0014**：推理后端自由化——默认 CPU + EP 可选 + 运行时分发
- **adr**：ADR-0015 区域级语义索引 + ADR-0016 检索原生 vision token 训练路线(Proposed)

### 🔧 杂项

- **ci**：一次性 VBSCRIPT 探针（验证 DISM 修复方案后即删）
- **ci**：探针脚本改纯 ASCII（powershell 5.1 无 BOM 中文乱码解析错误）
- **ci**：Wix 最小探针——runner 上直接跑 candle/light 抓真实报错
- **ci**：探针 YAML 修复——here-string 顶格破坏 block scalar，改数组拼接
- **ci**：Wix 探针 v2——windows 全量 tauri build -vv 抓 light 真实 stderr
- **ci**：Wix 探针 v3——32 位 cscript 与双位数 COM 注册状态（light 是 x86）
- **ci**：Wix 探针 v4——逐字节复刻 release windows 腿 + tauri-action + -vv
- **ci**：Tauri-action 加 -vv——light 失败时 stderr 可见（排查期保留）

### 🧪 实验

- **region-proto**：区域级语义索引原型——MobileSAM 分割 + Chinese-CLIP 区域编码 + 全局 k-means
- **region-proto**：首轮验证成功——5142 区域向量聚类出 32 个自然语义簇
- **region-proto**：在线 DP-means 增量聚类回放——K(N) 为生活事件驱动的阶梯,τ 是唯一旋钮
- **region-proto**：分层映射裁决——频域结构聚类→语义被否(NMI 0.18),语义低秩签名成立(PCA-64 纯度 0.83)
- **region-proto**：EXIF 时空调制初版——方向验证(K 响应时空覆盖 63→385,NMI 0.31→0.384),参数待标定
- **region-proto**：双空间共识评估——频域候选池召回不足(hit@10 0.147),PCA-64 无损定案(hit@50 反超)
- **region-proto**：知识锚点聚类原型——百科知识先验做聚类向导,120 中文概念锚点 81 簇带名
- **region-proto**：CLIP 偏移检验 + 层级/多轴验证——回应所有者三个方法论追问
- **region-proto**：Patch-MaxSim 晚交互实验——精排阶段注意力匹配 ×2.3 于余弦,所有者猜想验证
- **region-proto**：检索质量对比工具(retrieval_compare2)——区域流 vs 整图流 top-8 拼图
## v0.1.0-alpha.1（2026-09-05）

### ✨ 新增

- **app**：Tauri v2 + Vue 3 壳、specta 契约链路与颜色 token 设计基线
- **p1**：导入 → 缩略图 → 时间轴浏览 (#1)
- **p2**：中文语义搜索（模型下载/嵌入/检索） (#2)
- **p3**：混合检索（FTS trigram + 过滤 + RRF k=60）与 alpha 发布准备 (#3)

### 🐛 修复

- **ci**：质量门 job 补装 Linux 系统依赖（clippy 编译 tauri 需要）
- **release**：版本号回退纯数字 0.1.0（MSI 不接受字母预发布标识），alpha 语义由 Release prerelease 标志承担

### 📝 文档

- 项目文档基线（白皮书 v0.2、ADR-0001~0007、规范与模板）
- Whitepaper 文件名统一为小写 whitepaperv1.md（对齐全库引用）
- 落地 MVP 启动决策——起草 ADR-0008/0009/0010，白皮书 v0.3，规范与索引同步
- **developers**：00-setup / 03-ipc / 05-release，索引状态同步
- **developers**：Libheif vcpkg 构建集成 spike 结论与环境变量约定
- **adr**：ADR-0008/0009/0010 经项目所有者批准为 Accepted

### 🔧 杂项

- Cargo/pnpm 双 workspace 骨架，core 与 platform crate（ADR-0005）
- 质量门与三平台构建矩阵（无 e2e、无签名，ADR-0004/0010）
