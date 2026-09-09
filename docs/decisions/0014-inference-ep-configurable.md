# ADR-0014：推理 EP 运行时可配置（load-dynamic / 默认 CPU / 变体分发）

- 状态：Proposed（所有者 2026-09-09 拍板方向「默认 CPU + 设置自由选择 + 一键下载/导入」为执行授权；文档状态待追认）
- 日期：2026-09-09
- 决策者：项目所有者
- 相关：spec 0008、ADR-0006（模型版本锁定）、ADR-0011（数据根布局）、ADR-0013（可插拔索引）；编号备注：localplans 底稿曾暂定 0014 给 turbojpeg 提案，本 ADR 占用 0014，turbojpeg 顺延 0015

## 背景

实测（2026-09-09，RTX 4070 Laptop / 233 张 JPEG / release）：嵌入推理走 DirectML + int8 量化模型，nvidia-smi 确认 GPU 在算（利用率 74-100%），但 54s/229 张（236ms/张）远低于 4070 应有水平——根因是 DML 对 int8 QDQ 算子支持不全（部分节点回退 CPU + 每批主机↔显卡拷贝，ORT 日志 warn 可证）；批大小曲线 4→78.5s / **8→53.9s（甜点）** / 16→78.0s，参数层无便宜收益。CUDA EP 对 int8 QDQ 支持完善，预估可进 10s 内（待落地实测）。

现状约束：ort = 2.0-rc.13 **编译期静态链接**（download-binaries 按 cargo feature 下载对应变体静态库），EP 集合在编译期锁死为 DirectML→CPU；官方 onnxruntime 无「全 EP 合一」包，CUDA/DML 是不同变体产物。

所有者裁定：**默认最小配置（CPU），设置里写清楚给玩家自由选择；应用内一键下载或本地导入**——与「索引模型可自带可导入」（ADR-0013）同一产品哲学。

## 决策

1. **Windows 端 ort 切 `load-dynamic`**：动态库变体文件化——安装包随附 DML 变体（约 15MB）于 `runtime\dml\`，CUDA 变体按需获取；进程启动时按配置显式初始化（dylib 全局仅能加载一次 → **切换重启生效**）。mac/Linux 维持编译期链接现状（CoreML/CPU），缩小爆炸半径。
2. **默认 CPU**：`config.inference_ep` 缺省 `cpu`（最小配置原则）；升级用户的 DML 需在设置里手动开启（首启检测到 N 卡时卡内一次性引导，不弹窗）。
3. **EP 可配置 + 降级可见**：`EpKind { Cpu, DirectML, Cuda }`（mm-embed），session 按枚举注册 EP 链（cuda→dml→cpu shadowing）；所选 EP 失败自动降级 CPU，原因进 `inference_info` 与设置卡，**不改写用户配置**。
4. **运行时分发复用模型下载体系**：清单（sha256/size）+ `.part` 断点续传 + 防重入 + 本地 zip 导入（spec 0004 既有模式，spec 0008 §3.3/3.4）。
5. **同 EP 策略**：文本塔与视觉塔同一 EP，不分塔配置。
6. CUDA 设备枚举跳过虚拟显示适配器（IddCx），只认硬件适配器。

## 后果

### 积极

- N 卡用户获得数量级加速路径（预估 54s → 10s 内，落地实测后回填 spec 0008 §7）
- 零依赖默认：无独显/无驱动/离线用户开箱即用，不被运行时门槛拦截
- 模型与运行时统一「清单校验 + 断点续传 + 本地导入」心智，ADR-0013 的「用户自带模型」边界兑现
- 按需分发：无 N 卡用户永不下载 GB 级 CUDA 组件

### 消极 / 需关注

- load-dynamic 引入启动期 dylib 校验面（缺失/损坏 → 回退自带变体并提示），CI 打包矩阵需加 `runtime\dml\` 产物
- 默认 CPU 使升级用户（含所有者 4070 机器）建索引性能回退，需引导开启
- CUDA 运行时包解压后 GB 级、下载几百 MB，清单与 Release 托管是持续维护成本（每次 onnxruntime 升级需重打包）
- 动态加载后 ort 的 cargo feature（directml 等）仅余类型门控意义，实际可用性由变体文件决定——EP 注册必须容错降级，不允许 `unwrap`

## 备选方案

| 方案 | 结论 | 原因 |
| :--- | :--- | :--- |
| 编译期多 feature（cuda+directml 同链） | 否 | 官方无全 EP 合一运行时包，静态链接只能锁一个变体 |
| 维持静态 DML 默认 | 否 | 所有者裁定默认最小配置（CPU）；且 DML+int8 效率折扣实测显著 |
| TensorRT EP | 否 | 运行时 GB 级 + 按机建引擎，对分钟级后台索引收益不成比例 |
| fp16 模型 + 维持 DML | 缓（开放问题） | DML 对 fp16 支持完善，可消 int8 QDQ 回退；待模型侧产出 fp16 onnx 并实测（spec 0008 §8） |
| 只做 CUDA、不设默认 CPU | 否 | 违背「最小配置」裁定；无 N 卡用户被运行时门槛拦截 |

## 实测记录（2026-09-09/10，RTX 4070 Laptop / 233 张 JPEG@F: USB 盘 / release 构建 / 流水化嵌入 worker）

| 配置 | 嵌入墙钟 | 说明 |
| :--- | :--- | :--- |
| DirectML + int8（历史基线） | 54s~59.5s | DML 对 int8 QDQ 支持不全（部分算子回退 CPU）；批 4→78.5s / **8→53.9s（甜点）** / 16→78.0s |
| CPU EP（官方 cpu 变体 1.28.0） | 72.8s~77.6s | load-dynamic 链路全通；推理累计 66~73s |
| CUDA 变体 1.28.0 | 预检降级（未达推理） | **官方 gpu zip 不捆绑 cuDNN/cuBLAS**（ORT 1.19+ 政策），本机驱动 560 系无 CUDA 13 运行时 → `probe_provider_dll` 预检失败 → 降级 CPU，无段错误；CUDA 全速数据待装齐环境后回填 |
| DML 变体动态库 | 产物缺失 | 微软 DirectML nuget 冻结 1.24（API < 1.28 不可用）、pyke CDN 为加密私有格式 → 需源码自建（`--use_dml --build_dll`），产物入库前 DML 选择静默回退 CPU（1.28 对缺失 EP 不报错） |

工程要点：onnxruntime 1.28 对**注册失败的 session options 清理存在段错误**（上游 bug）——CUDA EP 必须先经 `probe_provider_dll` 预检（`LOAD_WITH_ALTERED_SEARCH_PATH` 使 cuDNN 从变体目录解析）再注册，禁止盲注册后降级。
