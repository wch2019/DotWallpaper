// DotWallpaper 壁纸工具 - Vue 3 入口
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "./styles/main.css";

createApp(App).use(createPinia()).mount("#app");
