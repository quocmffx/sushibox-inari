<script setup lang="ts">
// Tiny inline flag SVGs (emoji flags don't render on Windows).
defineProps<{ locale: 'en' | 'vi' }>()
</script>

<template>
  <!-- Vietnam: red field, yellow star -->
  <svg v-if="locale === 'vi'" viewBox="0 0 30 20" class="size-4 rounded-[2px] shrink-0">
    <rect width="30" height="20" fill="#da251d" />
    <path
      fill="#ff0"
      d="M15 5.2l1.27 3.9h4.1l-3.32 2.42 1.27 3.9L15 12.99l-3.32 2.42 1.27-3.9-3.32-2.41h4.1z"
    />
  </svg>

  <!-- English (US/UK simplified): blue field, white/red bars -->
  <svg v-else viewBox="0 0 30 20" class="size-4 rounded-[2px] shrink-0">
    <rect width="30" height="20" fill="#012169" />
    <path d="M0 0l30 20M30 0L0 20" stroke="#fff" stroke-width="3" />
    <path d="M0 0l30 20M30 0L0 20" stroke="#c8102e" stroke-width="1.6" />
    <path d="M15 0v20M0 10h30" stroke="#fff" stroke-width="5" />
    <path d="M15 0v20M0 10h30" stroke="#c8102e" stroke-width="3" />
  </svg>
</template>
