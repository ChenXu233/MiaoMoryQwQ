<script setup lang="ts">
// 照片格：库网格/瀑布流/搜索结果共用；显示缩略图 + hover 日期 + 可选徽标。
// 网格永远显示缩略图（缩略图≠原图是产品承诺，原图标识在 Lightbox）
import { computed } from "vue";
import type { AssetSummary } from "@miaomory/contracts";
import { assetSrc } from "../lib/ui";
import { formatDate } from "../lib/format";

const props = defineProps<{
  item: AssetSummary;
  /** 瀑布流：按宽高比撑高单元格 */
  fluid?: boolean;
  badge?: string;
  /** 未建语义索引：右下角呼吸点（规格 0007 §1 索引状态可见） */
  unindexed?: boolean;
}>();

const emit = defineEmits<{ open: [item: AssetSummary] }>();

const thumb = computed(() =>
  props.item.thumb_path ? assetSrc(props.item.thumb_path) : null,
);
const ratio = computed(() => {
  const { width, height } = props.item;
  if (props.fluid && width && height) return `${width} / ${height}`;
  return undefined;
});
</script>

<template>
  <button
    class="tile"
    :style="ratio ? { aspectRatio: ratio } : undefined"
    :aria-label="`查看照片（${formatDate(item.taken_at)}）`"
    @click="emit('open', item)"
  >
    <span class="ph">
      <img v-if="thumb" :src="thumb" alt="" loading="lazy" />
    </span>
    <span v-if="unindexed" class="idx-dot" title="建立索引中" />
    <span class="date-pill">{{ formatDate(item.taken_at) }}</span>
    <span v-if="badge" class="badge">{{ badge }}</span>
  </button>
</template>
