"""导出 MobileSAM ONNX(切片 A0):编码器(整图一次)+ 点提示解码器(每批点一次)。

- encoder:  [1,3,1024,1024] → [1,256,64,64] image embedding
- decoder:  点批(≤64 点/标签)→ low-res masks [1,N,256,256] + stability/iou 分数
  (orig_im_size 固定传 256×256,保持低分辨率输出;Rust 端自行换算到原图坐标)
- 运行: .venv/Scripts/python.exe export_sam_onnx.py --weights mobile_sam.pt --out onnx
"""
import argparse
import os

import torch
from mobile_sam import sam_model_registry
from mobile_sam.utils.onnx import SamOnnxModel


class EncoderExport(torch.nn.Module):
    def __init__(self, sam):
        super().__init__()
        self.image_encoder = sam.image_encoder

    def forward(self, x):
        return self.image_encoder(x)


class DecoderExport(torch.nn.Module):
    """MobileSAM SamOnnxModel 签名:点坐标+标签输入,返回 (upscaled_masks, scores, low_res)。
    orig 传 [256,256] 保持低分辨率输出(面积/bbox 在低分辨率域计算,Rust 端换算到原图)。"""

    def __init__(self, sam):
        super().__init__()
        self.model = SamOnnxModel(sam, return_single_mask=False, use_stability_score=False,
                                  return_extra_metrics=False)

    def forward(self, image_embeddings, point_coords, point_labels, mask_input, has_mask_input,
                orig_im_size):
        upscaled, scores, low = self.model(
            image_embeddings, point_coords, point_labels, mask_input, has_mask_input,
            orig_im_size,
        )
        return upscaled, scores


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--weights", default="mobile_sam.pt")
    ap.add_argument("--model-type", default="vit_t")
    ap.add_argument("--batch", type=int, default=16)
    ap.add_argument("--out", default="onnx")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)

    sam = sam_model_registry[args.model_type](checkpoint=args.weights)
    sam.eval()

    enc = EncoderExport(sam)
    torch.onnx.export(
        enc, torch.randn(1, 3, 1024, 1024),
        os.path.join(args.out, "sam_encoder.onnx"),
        input_names=["images"], output_names=["embeddings"],
        opset_version=17, do_constant_folding=True,
    )
    print("encoder exported")

    dec = DecoderExport(sam)
    batch = args.batch
    demo = {
        "image_embeddings": torch.randn(1, 256, 64, 64),
        "point_coords": torch.rand(1, batch, 2) * 1024,
        "point_labels": torch.ones(1, batch),
        "mask_input": torch.zeros(1, 1, 256, 256),
        "has_mask_input": torch.zeros(1),
        "orig_im_size": torch.tensor([256.0, 256.0]),
    }
    torch.onnx.export(
        dec,
        (demo["image_embeddings"], demo["point_coords"], demo["point_labels"],
         demo["mask_input"], demo["has_mask_input"], demo["orig_im_size"]),
        os.path.join(args.out, "sam_decoder.onnx"),
        input_names=list(demo.keys()),
        output_names=["masks", "scores"],
        opset_version=17, do_constant_folding=True,
    )
    print("decoder exported")


if __name__ == "__main__":
    main()
