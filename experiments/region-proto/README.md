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

## 结论

(跑完后填写:簇是否自然浮现、大小分布、代表区域质量、下一步判断)
