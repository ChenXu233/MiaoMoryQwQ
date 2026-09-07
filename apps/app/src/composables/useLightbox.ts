// 灯箱状态（单一实例）：库页与搜索结果共用；A1 起支持离线回退（见 Lightbox.vue）
import { ref } from "vue";
import type { AssetSummary } from "@miaomory/contracts";

const items = ref<AssetSummary[]>([]);
const index = ref<number | null>(null);

export function useLightbox() {
  function open(list: AssetSummary[], i: number) {
    items.value = list;
    index.value = i;
  }
  function close() {
    index.value = null;
  }
  function navigate(i: number) {
    index.value = i;
  }
  return { items, index, open, close, navigate };
}
