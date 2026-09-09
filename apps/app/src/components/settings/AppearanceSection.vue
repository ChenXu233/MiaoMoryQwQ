<script setup lang="ts">
// 设置 · 外观（spec 0006 §3.0 分类一）
import { useTheme } from "../../composables/useTheme";

const { theme, setTheme } = useTheme();
</script>

<template>
  <div class="card">
    <div class="set-row">
      <div class="set-info">
        <div class="set-t">主题</div>
        <div class="set-d">亮色 / 暗色，立即生效</div>
      </div>
      <div class="seg" role="group" aria-label="主题">
        <button :class="{ on: theme === 'light' }" @click="setTheme('light')">亮色</button>
        <button :class="{ on: theme === 'dark' }" @click="setTheme('dark')">暗色</button>
      </div>
    </div>
    <div class="hr" />
    <div class="set-row">
      <div class="set-info">
        <div class="set-t">语言</div>
        <div class="set-d">界面显示语言（LATER：多语言未实现）</div>
      </div>
      <div class="seg" role="group" aria-label="语言">
        <button class="on">简体中文</button>
        <button disabled>English</button>
      </div>
    </div>
  </div>
</template>
