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
