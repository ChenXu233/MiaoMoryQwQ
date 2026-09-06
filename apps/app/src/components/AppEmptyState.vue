<script setup lang="ts">
// 空态容器：主链路空态/引导统一入口（体验哲学：状态完备）
defineProps<{ title: string; description?: string }>();
</script>

<template>
  <div class="mx-auto flex max-w-md flex-col items-center gap-3 text-center">
    <h1 class="text-2xl font-semibold text-fg">{{ title }}</h1>
    <p v-if="description" class="text-sm leading-6 text-muted">{{ description }}</p>
    <div class="mt-3">
      <slot name="action" />
    </div>
  </div>
</template>
