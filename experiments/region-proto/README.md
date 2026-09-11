# region-proto:区域级语义索引 Python 原型

验证构想(2026-09-11 与所有者讨论):**自然区域分离 → 区域 embedding → 跨库全局聚类(自然语义类)→ 区域级晚交互检索** 的前两步半,拿 F:\相册 229 张真实照片离线验证簇质量。

## 管线

```
照片 → MobileSAM 全图分割(自然区域 mask)
     → 区域裁剪(bbox+12% padding)
     → Chinese-CLIP ViT-B/16 int8 图像塔(复用 .devdata/models,零训练)
     → 512 维区域向量(L2 归一)→ regions.npz
     → 全局 k-means(k=32)→ 簇可视化拼图 + 统计
```

对应文献:Segment Anything(ICCV 2023)/ RegionCLIP(CVPR 2022)/ ViLD(ICLR 2022)/
SlotCon(NeurIPS 2022)/ ColPali late interaction。缺口 = 增量聚类下的簇稳定性(组合新颖点)。

## 复现

```powershell
python -m venv .venv
.venv\Scripts\pip install torch --index-url https://download.pytorch.org/whl/cpu
.venv\Scripts\pip install timm scikit-learn pillow opencv-python-headless
.venv\Scripts\pip install git+https://github.com/ChaoningZhang/MobileSAM.git
# 权重:mobile_sam.pt 放本目录(GitHub ChaoningZhang/MobileSAM weights)
.venv\Scripts\python extract.py --photos "F:\相册\2022.10-2024.7\相机" --limit 229
.venv\Scripts\python cluster.py --k 32
```

## 结论(2026-09-11,首轮验证)

**构想成立。** 229 张(228 张可读)→ **5142 个区域向量**(MobileSAM vit_t 分割 + Chinese-CLIP
int8 区域编码,平均 22.6 区域/图,CPU 58 分钟)。k=32 全局 k-means,**无空簇,簇大小
27~334(均值 161),抽查簇全部语义一致且是"这本相册的自然类别"**:

| 簇 | n | 语义 |
|---|---|---|
| cluster 0 | 293 | 天空/云(含晚霞、树影天空) |
| cluster 1 | 174 | 教室人物(蓝白校服、书包、课桌) |
| cluster 8 | 166 | 树叶/植被特写 |
| cluster 16 | 145 | 绿篱/花坛/人工绿化边界 |
| cluster 27 | 180 | 山林远景(松树、山坡) |

要点:
- 区域级 embedding 的区分度肉眼可见地高于整图向量——簇内一致性不靠阈值,是聚类自然涌现的
- 这些簇 = "这本相册的原型空间":通用 CLIP 做不到(它没有这批数据的分布),完全零训练
- 全部 CPU 跑通:MobileSAM ~10s/张 + CLIP ~0.1s/区域;换 DirectML/GPU 可降一个量级

## 下一步候选

1. 检索验证:区域向量 + MaxSim 聚合(每图得分 = Σ_query max_region),对比整图 KNN 的窄带问题
2. 增量聚类(stream k-means / 原型漂移)——组合空白点,ADR 核心议题
3. 簇作为浏览维度(点开"天空"簇看全部天空照)
4. k 敏感度(16/64/128)与层次聚类(粗簇→细簇)

## 增量聚类实验(2026-09-11,dpmeans_online.py)

**在线 DP-means**(距最近原型 1-cos > τ → 诞生新原型;否则加权均值漂移 1/count)
按拍摄序回放 5142 区域,τ 扫描:

| τ | 最终 K | top1 簇占比 | 单例簇 |
|---|---|---|---|
| 0.18 | 219 | 16% | **101**(过碎) |
| 0.22 | 85 | 26% | 34 |
| 0.26 | 46 | 27% | 16 |
| 0.30 | 25 | **45%**(过吞) | 12 |
| 0.35 | 17 | 65% | 8 |

**K(N) 曲线形状(关键发现)**:不是平滑 log N,而是**"平台 + 跳跃"的阶梯**——
平稳生活期原型几乎不增(τ=0.30 时 15 个簇维持了 100 张),生活变化期(2024 段)
一批新语义类集中诞生。U·log N 是长期平均的粗近似(rmse 14~17%),机制上
**K(N) 由生活事件驱动、τ 是唯一旋钮**(所有者的 U 即由此而来)。

设计推论:
1. 在线生灭 + 周期重排的混合结构正好匹配阶梯动态:跳跃期在线准入快速建簇,平台期零漂移
2. 纯在线必然产碎片(τ=0.18 时 50% 簇是单例)——**衰退合并(低权重老原型并入最近邻)
   与周期 mini-batch 重排是必需件**,不是可选项
3. 推荐工作点 τ≈0.22~0.26(余弦归簇门限 0.74~0.78),K 随库增长自然上行
