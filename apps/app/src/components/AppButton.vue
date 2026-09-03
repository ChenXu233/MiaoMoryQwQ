<script setup lang="ts">
// 极简主操作按钮（ADR-0009：不引组件库，自建基础件）
defineProps<{ disabled?: boolean }>();

const emit = defineEmits<{ click: [] }>();
</script>

<template>
  <button
    type="button"
    class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-accent-fg transition-colors hover:opacity-90 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50"
    :disabled="disabled"
    @click="emit('click')"
  >
    <slot />
  </button>
</template>
