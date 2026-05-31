export default defineNuxtConfig({
  modules: ['@nuxt/ui'],
  ssr: false,
  css: ['~/assets/css/main.css'],
  colorMode: {
    classSuffix: '',
    preference: 'system',
    fallback: 'dark',
    storageKey: 'inari-color-mode',
  },
  nitro: {
    output: {
      publicDir: 'dist',
    },
  },
})
