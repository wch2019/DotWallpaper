<script setup lang="ts">
import { useDialog, useMessage } from "naive-ui";
import { bindNaiveHost } from "../lib/naive-host";

// 将 Naive UI 的 message / dialog 注入全局宿主，供 Pinia store 等非组件上下文调用
bindNaiveHost(useMessage(), useDialog());
</script>

<template>
  <slot />
</template>
