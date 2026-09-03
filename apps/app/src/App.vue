<script setup lang="ts">
import { ref } from "vue";
import { commands } from "@miaomory/contracts";
import AppButton from "./components/AppButton.vue";
import AppEmptyState from "./components/AppEmptyState.vue";

const greeting = ref("");
const busy = ref(false);
const failed = ref(false);

// P0 契约链路验证：调用 Rust 命令并显示返回；P1 将替换为真实的空态引导
async function checkContract() {
  busy.value = true;
  failed.value = false;
  try {
    greeting.value = await commands.greet("MiaoMory");
  } catch {
    failed.value = true;
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <main class="flex h-full flex-col items-center justify-center gap-6 bg-bg text-fg">
    <AppEmptyState
      title="MiaoMory"
      description="个人媒体语义索引引擎。当前为 P0 骨架：点击下方按钮验证 IPC 契约链路，通过后进入 P1 导入与浏览。"
    >
      <template #action>
        <AppButton :disabled="busy" @click="checkContract">
          {{ busy ? "验证中…" : "验证 IPC 契约" }}
        </AppButton>
      </template>
    </AppEmptyState>
    <p v-if="greeting" class="text-sm text-success">{{ greeting }}</p>
    <p v-if="failed" class="text-sm text-danger">调用失败，请查看日志（AppData/MiaoMory/logs）。</p>
  </main>
</template>
