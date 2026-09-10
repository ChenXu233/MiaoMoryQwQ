<script setup lang="ts">
// 确认弹窗视图(数据源 useConfirm 单例):玻璃卡 + 危险红按钮 + Enter/Esc/遮罩点击。
import { onBeforeUnmount, onMounted } from "vue";
import { useConfirm } from "../../composables/useConfirm";

const { state, settle } = useConfirm();

function onKey(e: KeyboardEvent) {
  if (!state.value) return;
  if (e.key === "Escape") settle(false);
  else if (e.key === "Enter") {
    e.preventDefault();
    settle(true);
  }
}

onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Transition name="fade">
    <div v-if="state" class="confirm-scrim" @click="settle(false)">
      <div class="confirm-card" role="alertdialog" aria-modal="true" :aria-label="state.title" @click.stop>
        <div class="confirm-t">{{ state.title }}</div>
        <div class="confirm-m">{{ state.message }}</div>
        <div class="confirm-ops">
          <button class="btn-glass btn-outline-glass" @click="settle(false)">
            {{ state.cancelLabel ?? "取消" }}
          </button>
          <button
            class="btn-glass"
            :class="state.danger ? 'btn-danger' : 'btn-primary'"
            @click="settle(true)"
          >
            {{ state.okLabel ?? "确定" }}
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>
