<script setup lang="ts">
// 右键菜单视图(数据源 useContextMenu 单例):玻璃小卡,点击动作后自动关闭
// (关闭器是全局 capture once click,动作执行前先同步关闭避免竞态)。
import { useContextMenu } from "../../composables/useContextMenu";

const { pos, items, close } = useContextMenu();

function run(index: number) {
  const item = items.value[index];
  close();
  // 让关闭事件先落定,再执行动作(动作可能自身弹确认框)
  window.setTimeout(() => item?.action(), 0);
}
</script>

<template>
  <div
    v-if="pos"
    class="ctx-menu"
    :style="{ left: pos.x + 'px', top: pos.y + 'px' }"
    role="menu"
  >
    <button
      v-for="(it, i) in items"
      :key="it.label"
      class="ctx-item"
      :class="{ danger: it.danger }"
      role="menuitem"
      @click="run(i)"
    >
      {{ it.label }}
    </button>
  </div>
</template>
