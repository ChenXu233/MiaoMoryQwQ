<script setup lang="ts">
// 空态引导（体验规格 0001 §3）
import AppButton from "./AppButton.vue";
import AppEmptyState from "./AppEmptyState.vue";

defineEmits<{ pickFolder: [] }>();
</script>

<template>
  <AppEmptyState
    title="你的照片，值得被找到"
    description="选择照片文件夹，MiaoMory 自动整理成可搜索的回忆。所有处理都在本机完成。"
  >
    <template #action>
      <AppButton @click="$emit('pickFolder')">选择照片文件夹</AppButton>
    </template>
  </AppEmptyState>
</template>
