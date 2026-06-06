<script setup lang="ts">
interface Service {
  kind: string
  name: string
  version: string
  state: 'running' | 'stopped'
  pid: number | null
  port: number
}
interface Config {
  flavor: string
  ports: Record<string, number>
  php?: { version: string; cgi_port: number }
  sites: { name: string; root: string }[]
}

// i18n
const { locale, toggle: toggleLocale, load: loadLocale, t } = useLocale()

// Theme — system / light / dark, picked once at page level (Page Theme Lock).
// useColorMode persists the choice; no forced override (which caused a flash).
const colorMode = useColorMode()
const theme = computed({
  get: () => (colorMode.preference === 'light' || colorMode.preference === 'dark')
    ? colorMode.preference
    : 'system',
  set: (val: 'system' | 'light' | 'dark') => { colorMode.preference = val },
})
const themeOptions = computed(() => [
  { value: 'system', label: t('themeSystem') },
  { value: 'light',  label: t('themeLight') },
  { value: 'dark',   label: t('themeDark') },
])

// Data
const { data: status, refresh, pending, error } = await useFetch<{ services: Service[] }>('/api/status', {
  default: () => ({ services: [] }),
})
const { data: config, refresh: refreshConfig } = await useFetch<Config>('/api/config', {
  default: () => ({ flavor: 'default', ports: {}, sites: [] }),
})

// Settings (live, client-side). Persisted in localStorage.
const settingsOpen = ref(false)
const autoRefresh = ref(true)
const refreshSec = ref(5)
const refreshMs = computed(() => refreshSec.value * 1000)

// Load persisted settings
onMounted(() => {
  loadLocale()
  const saved = localStorage.getItem('inari.settings')
  if (saved) {
    try {
      const s = JSON.parse(saved)
      if (typeof s.autoRefresh === 'boolean') autoRefresh.value = s.autoRefresh
      if (typeof s.refreshSec === 'number') refreshSec.value = s.refreshSec
    }
    catch { /* ignore */ }
  }
})
watch([autoRefresh, refreshSec], ([ar, rs]) => {
  localStorage.setItem('inari.settings', JSON.stringify({ autoRefresh: ar, refreshSec: rs }))
})

// Auto-refresh timer (resets when interval or toggle changes)
let autoTimer: ReturnType<typeof setInterval> | undefined
function resetTimer() {
  if (autoTimer) clearInterval(autoTimer)
  if (autoRefresh.value) autoTimer = setInterval(() => refresh(), refreshMs.value)
}
watch([autoRefresh, refreshMs], resetTimer, { immediate: true })
onUnmounted(() => { if (autoTimer) clearInterval(autoTimer) })

// ── Server settings (settings.json via /api/settings) ──────────────────
interface ServerSettings {
  ports: { panel: number | null, web: number | null, mysql: number | null, redis: number | null }
  sites: { name: string, root: string, index?: string | null }[] | null
  autostart: string[] | null
  run_at_startup?: boolean
  start_minimized?: boolean
}
interface SaveSettingsResponse {
  ok: boolean
  note?: string
  error?: string
  settings?: ServerSettings
  config?: Config
}
const settingsTab = ref<'general' | 'services' | 'php'>('general')
const savingSettings = ref(false)
const settingsNotice = ref<{ text: string; ok: boolean } | null>(null)
// Editable form models (seeded from /api/config effective values)
const portWeb = ref<number>(8080)
const portMysql = ref<number>(3307)
const portRedis = ref<number>(6380)
const docRoot = ref<string>('sites/default')
const autostartKinds = ref<Record<string, boolean>>({ nginx: false, mysql: false, redis: false })
const runAtStartup = ref<boolean>(false)
const startMinimized = ref<boolean>(false)

async function loadServerSettings() {
  // Pull fresh effective config first so Settings and the home page never drift.
  await refreshConfig()
  try {
    const s = await $fetch<ServerSettings>('/api/settings')

    // Seed editable fields from persisted settings when present; otherwise use
    // the effective config (flavor + overlay) returned by /api/config.
    portWeb.value = s.ports?.web ?? config.value?.ports?.web ?? 8080
    portMysql.value = s.ports?.mysql ?? config.value?.ports?.mysql ?? 3307
    portRedis.value = s.ports?.redis ?? config.value?.ports?.redis ?? 6380
    docRoot.value = s.sites?.[0]?.root ?? config.value?.sites?.[0]?.root ?? 'sites/default'

    const set = new Set(s.autostart ?? [])
    for (const k of Object.keys(autostartKinds.value)) autostartKinds.value[k] = set.has(k)
    runAtStartup.value = s.run_at_startup ?? false
    startMinimized.value = s.start_minimized ?? false
  }
  catch {
    // Fallback to effective config if settings.json cannot be read.
    portWeb.value = config.value?.ports?.web ?? 8080
    portMysql.value = config.value?.ports?.mysql ?? 3307
    portRedis.value = config.value?.ports?.redis ?? 6380
    docRoot.value = config.value?.sites?.[0]?.root ?? 'sites/default'
  }
}

async function saveServerSettings() {
  savingSettings.value = true
  settingsNotice.value = null
  const autostart = Object.entries(autostartKinds.value)
    .filter(([, on]) => on)
    .map(([k]) => k)
  const body: ServerSettings = {
    ports: { panel: null, web: portWeb.value, mysql: portMysql.value, redis: portRedis.value },
    sites: [{ name: 'default', root: docRoot.value, index: 'index.php' }],
    autostart,
    run_at_startup: runAtStartup.value,
    start_minimized: startMinimized.value,
  }
  try {
    const res = await $fetch<SaveSettingsResponse>('/api/settings', { method: 'POST', body })
    if (res.ok) {
      const msg = res.note ?? t('settingsSaved')
      settingsNotice.value = { text: msg, ok: true }
      pushMessage(msg, true)

      // The save response is authoritative. Apply it immediately so Settings
      // and the home card cannot disagree while Nuxt's useFetch cache catches up.
      if (res.config) config.value = res.config
      if (res.settings) {
        portWeb.value = res.settings.ports?.web ?? config.value?.ports?.web ?? 8080
        portMysql.value = res.settings.ports?.mysql ?? config.value?.ports?.mysql ?? 3307
        portRedis.value = res.settings.ports?.redis ?? config.value?.ports?.redis ?? 6380
        docRoot.value = res.settings.sites?.[0]?.root ?? config.value?.sites?.[0]?.root ?? 'sites/default'
        const set = new Set(res.settings.autostart ?? [])
        for (const k of Object.keys(autostartKinds.value)) autostartKinds.value[k] = set.has(k)
        runAtStartup.value = res.settings.run_at_startup ?? false
        startMinimized.value = res.settings.start_minimized ?? false
      }

      await refreshConfig()
      await refresh()
    } else {
      const msg = `${t('settings')}: ${res.error ?? t('saveFailed')}`
      settingsNotice.value = { text: msg, ok: false }
      pushMessage(msg, false)
    }
  }
  catch (e: any) {
    const msg = `${t('settings')}: ${e?.data?.error ?? t('saveFailed')}`
    settingsNotice.value = { text: msg, ok: false }
    pushMessage(msg, false)
  }
  finally {
    savingSettings.value = false
  }
}

function openSettings() {
  settingsNotice.value = null
  loadServerSettings()
  settingsOpen.value = true
}

const phpVersion = computed(() => config.value?.php?.version ?? '8.4.21')
const phpCgiPort = computed(() => config.value?.php?.cgi_port ?? 9000)

// Aggregate state for bulk-action enable/disable
const runningCount = computed(() => status.value?.services?.filter(s => s.state === 'running').length ?? 0)
const allRunning  = computed(() => {
  const svcs = status.value?.services ?? []
  return svcs.length > 0 && svcs.every(s => s.state === 'running')
})
const noneRunning = computed(() => runningCount.value === 0)
// Open-site only makes sense while nginx serves; backend stays authoritative.
const nginxRunning = computed(() =>
  status.value?.services?.some(s => s.kind === 'nginx' && s.state === 'running') ?? false)

// Message box — keeps a few recent lines
const messages = ref<{ text: string; ok: boolean }[]>([])
onMounted(() => { if (messages.value.length === 0) messages.value.push({ text: t('ready'), ok: true }) })
const pushMessage = (text: string, ok: boolean) => {
  messages.value.push({ text, ok })
  if (messages.value.length > 6) messages.value.shift()
}

const pastTense = (a: string) =>
  a === 'stop' ? t('stop') : a === 'start' ? t('start') : t('restart')

const runAction = async (kind: string, name: string, action: 'start' | 'stop' | 'restart') => {
  try {
    const res = await $fetch<{ ok: boolean; error?: string; pid?: number }>(
      `/api/services/${kind}/${action}`, { method: 'POST' },
    )
    if (res.ok) pushMessage(`${name} — ${pastTense(action)}${res.pid ? ` (pid ${res.pid})` : ''}`, true)
    else pushMessage(`${name}: ${res.error ?? t('actionFailed')}`, false)
  } catch (e: any) {
    pushMessage(`${name}: ${e?.data?.error ?? e?.message ?? t('requestFailed')}`, false)
  }
}

// Per-service
const pendingKind = ref<string | null>(null)
const serviceAction = async (kind: string, name: string, action: 'start' | 'stop' | 'restart') => {
  pendingKind.value = `${kind}:${action}`
  await runAction(kind, name, action)
  pendingKind.value = null
  await refresh()
}

// Bulk
const bulkPending = ref<string | null>(null)
// Dependency-aware user-facing order. PHP-CGI is managed internally by nginx.
const START_ORDER = ['mysql', 'redis', 'nginx']
const STOP_ORDER = ['nginx', 'redis', 'mysql']
const bulkAction = async (action: 'start' | 'stop' | 'restart') => {
  bulkPending.value = action
  const order = action === 'start' ? START_ORDER : STOP_ORDER
  const svcs = [...(status.value?.services ?? [])].sort(
    (a, b) => order.indexOf(a.kind) - order.indexOf(b.kind),
  )
  for (const svc of svcs) {
    if (action === 'start' && svc.state === 'running') continue
    if (action !== 'start' && svc.state !== 'running') continue
    await runAction(svc.kind, svc.name, action)
  }
  bulkPending.value = null
  await refresh()
}

const openTarget = async (target: string, label: string) => {
  try {
    const res = await $fetch<{ ok: boolean; error?: string }>(`/api/open/${target}`, { method: 'POST' })
    pushMessage(res.ok ? t('opened', { label }) : `${label}: ${res.error ?? t('actionFailed')}`, res.ok)
  } catch (e: any) {
    pushMessage(`${label}: ${e?.data?.error ?? t('actionFailed')}`, false)
  }
}

// ── AI / MCP server — start/stop an MCP endpoint for an AI agent ────────
interface McpStatus { running: boolean; port: number; url: string }
interface McpActionResponse { ok: boolean; port?: number; url?: string; error?: string }
const mcpOn = ref(false)
const mcpUrl = ref('')
const mcpPending = ref(false)
const mcpCopied = ref(false)

async function loadMcp() {
  try {
    const s = await $fetch<McpStatus>('/api/mcp')
    mcpOn.value = s.running
    mcpUrl.value = s.url ?? ''
  } catch { /* backend unreachable — leave toggle off */ }
}
onMounted(loadMcp)

async function toggleMcp(on: boolean) {
  // Optimistic flip; revert on failure.
  mcpPending.value = true
  try {
    if (on) {
      const res = await $fetch<McpActionResponse>('/api/mcp/start', { method: 'POST' })
      if (res.ok) {
        mcpOn.value = true
        mcpUrl.value = res.url ?? ''
        pushMessage(`${t('mcpStarted')}${res.url ? ` · ${res.url}` : ''}`, true)
      } else {
        mcpOn.value = false
        pushMessage(`${t('mcpFailed')}: ${res.error ?? t('actionFailed')}`, false)
      }
    } else {
      const res = await $fetch<McpActionResponse>('/api/mcp/stop', { method: 'POST' })
      if (res.ok) {
        mcpOn.value = false
        mcpUrl.value = ''
        pushMessage(t('mcpStopped'), true)
      } else {
        mcpOn.value = true
        pushMessage(`${t('mcpFailed')}: ${res.error ?? t('actionFailed')}`, false)
      }
    }
  } catch (e: any) {
    mcpOn.value = !on
    pushMessage(`${t('mcpFailed')}: ${e?.data?.error ?? e?.message ?? t('requestFailed')}`, false)
  } finally {
    mcpPending.value = false
  }
}

async function copyMcpUrl() {
  if (!mcpUrl.value) return
  try {
    await navigator.clipboard.writeText(mcpUrl.value)
    mcpCopied.value = true
    setTimeout(() => { mcpCopied.value = false }, 1200)
  } catch { /* clipboard blocked — ignore */ }
}
</script>

<template>
  <div class="h-screen bg-default p-2 flex flex-col">
    <div class="flex flex-col flex-1 min-h-0 space-y-2">

      <!-- Header -->
      <div class="flex items-center justify-between px-1">
        <div class="flex items-baseline gap-1.5 min-w-0">
          <span class="text-[10px] text-muted leading-none shrink-0">SushiBox</span>
          <h1 class="text-sm font-bold text-highlighted leading-none truncate">Inari</h1>
        </div>
        <div class="flex items-center gap-0.5 shrink-0">
          <UButton
            :icon="colorMode.value === 'dark' ? 'i-lucide-moon' : 'i-lucide-sun'"
            color="neutral" variant="ghost" size="xs"
            :aria-label="t('theme')"
            @click="theme = colorMode.value === 'dark' ? 'light' : 'dark'"
          />
          <UButton
            icon="i-lucide-refresh-cw" :loading="pending"
            color="neutral" variant="ghost" size="xs"
            :aria-label="t('refresh')" @click="refresh"
          />
          <UButton
            icon="i-lucide-settings" color="neutral" variant="ghost" size="xs"
            :aria-label="t('settings')" @click="openSettings"
          />
        </div>
      </div>

      <!-- API error -->
      <UAlert
        v-if="error"
        icon="i-lucide-wifi-off" color="error" variant="soft"
        :title="t('cannotReachApi')" :ui="{ title: 'text-xs', root: 'p-1.5' }"
      />

      <!-- Runtime -->
      <UCard :ui="{ root: 'rounded-lg', body: 'p-0 sm:p-0', header: 'px-3 py-2 sm:px-3 sm:py-2', footer: 'p-2 sm:p-2' }">
        <template #header>
          <div class="flex items-center justify-between">
            <span class="text-xs font-semibold text-highlighted">{{ t('runtime') }}</span>
            <span class="text-[10px] text-muted">{{ runningCount }}/{{ status?.services?.length ?? 0 }}</span>
          </div>
        </template>
        <div>
          <div
            v-for="svc in status?.services"
            :key="svc.kind"
            class="group flex items-center gap-2.5 px-3 py-2 hover:bg-elevated/40 transition-colors"
          >
            <span
              class="size-2 rounded-full shrink-0 ring-2 transition-colors"
              :class="svc.state === 'running'
                ? 'bg-primary ring-primary/20'
                : 'bg-muted ring-transparent border border-default'"
            />
            <div class="flex-1 min-w-0">
              <p class="text-xs font-medium text-highlighted leading-tight">
                {{ svc.name }}
                <span class="text-[10px] font-normal text-dimmed">{{ svc.version }}</span>
              </p>
              <p class="text-[10px] font-mono text-dimmed leading-tight">
                :{{ svc.port }}<span v-if="svc.kind === 'nginx'"> · PHP {{ phpVersion }}</span><span v-if="svc.pid"> · {{ svc.pid }}</span>
              </p>
            </div>
            <!-- Stopped: single Start. Running: Stop + Restart as quiet icons. -->
            <div class="flex items-center gap-1 shrink-0">
              <UButton
                v-if="svc.state !== 'running'"
                color="primary" variant="soft" size="xs"
                :loading="pendingKind === `${svc.kind}:start`"
                :disabled="!!pendingKind || !!bulkPending"
                @click="serviceAction(svc.kind, svc.name, 'start')"
              >{{ t('start') }}</UButton>
              <template v-else>
                <UButton
                  icon="i-lucide-rotate-cw" color="neutral" variant="ghost" size="xs"
                  :loading="pendingKind === `${svc.kind}:restart`"
                  :disabled="!!pendingKind || !!bulkPending"
                  :aria-label="t('restart')" :title="t('restart')"
                  @click="serviceAction(svc.kind, svc.name, 'restart')"
                />
                <UButton
                  icon="i-lucide-square" color="neutral" variant="ghost" size="xs"
                  :loading="pendingKind === `${svc.kind}:stop`"
                  :disabled="!!pendingKind || !!bulkPending"
                  :aria-label="t('stop')" :title="t('stop')"
                  @click="serviceAction(svc.kind, svc.name, 'stop')"
                />
              </template>
            </div>
          </div>
        </div>
        <template #footer>
          <div class="flex items-center gap-1.5">
            <UButton
              color="primary" variant="solid" size="xs" block
              icon="i-lucide-play"
              :loading="bulkPending === 'start'" :disabled="!!bulkPending || !!pendingKind || allRunning"
              @click="bulkAction('start')"
            >{{ t('startAll') }}</UButton>
            <UButton
              icon="i-lucide-rotate-cw" color="neutral" variant="soft" size="xs"
              :loading="bulkPending === 'restart'" :disabled="!!bulkPending || !!pendingKind || noneRunning"
              :aria-label="t('restartAll')" :title="t('restartAll')"
              @click="bulkAction('restart')"
            />
            <UButton
              icon="i-lucide-square" color="neutral" variant="soft" size="xs"
              :loading="bulkPending === 'stop'" :disabled="!!bulkPending || !!pendingKind || noneRunning"
              :aria-label="t('stopAll')" :title="t('stopAll')"
              @click="bulkAction('stop')"
            />
          </div>
        </template>
      </UCard>

      <!-- AI / MCP — start an MCP endpoint an AI agent can drive the stack with. -->
      <div class="rounded-lg border border-default bg-muted/40 px-3 py-2">
        <div class="flex items-center gap-2.5">
          <UIcon name="i-lucide-bot" class="size-4 text-muted shrink-0" />
          <div class="flex-1 min-w-0">
            <p class="text-xs font-medium text-highlighted leading-tight">{{ t('aiMcp') }}</p>
            <p class="text-[10px] text-muted leading-tight">{{ t('aiMcpHint') }}</p>
          </div>
          <USwitch
            v-model="mcpOn"
            size="sm"
            :loading="mcpPending"
            :disabled="mcpPending"
            @update:model-value="toggleMcp"
          />
        </div>
        <div
          v-if="mcpOn && mcpUrl"
          class="mt-2 flex items-center gap-1.5 pl-6"
        >
          <code class="text-[10px] font-mono text-dimmed truncate">{{ mcpUrl }}</code>
          <UButton
            :icon="mcpCopied ? 'i-lucide-check' : 'i-lucide-copy'"
            color="neutral" variant="ghost" size="xs"
            :aria-label="t('copy')" :title="mcpCopied ? t('copied') : t('copy')"
            @click="copyMcpUrl"
          />
        </div>
      </div>

      <!-- Dev shortcuts — turn the demo/test loop into buttons, no terminal. -->
      <div class="grid grid-cols-4 gap-1.5">
        <UButton
          icon="i-lucide-external-link" color="neutral" variant="outline" size="xs" block
          :disabled="!nginxRunning"
          :title="t('openSite')"
          @click="openTarget('web', t('openSite'))"
        >{{ t('openSite') }}</UButton>
        <UButton
          icon="i-lucide-database" color="neutral" variant="outline" size="xs" block
          :disabled="!nginxRunning"
          :title="t('openAdminer')"
          @click="openTarget('adminer', t('openAdminer'))"
        >{{ t('openAdminer') }}</UButton>
        <UButton
          icon="i-lucide-folder" color="neutral" variant="outline" size="xs" block
          :title="t('siteFolder')"
          @click="openTarget('site', t('siteFolder'))"
        >{{ t('siteFolder') }}</UButton>
        <UButton
          icon="i-lucide-scroll-text" color="neutral" variant="outline" size="xs" block
          :title="t('logsFolder')"
          @click="openTarget('logs', t('logsFolder'))"
        >{{ t('logsFolder') }}</UButton>
      </div>

      <!-- Message box — grows to fill remaining height -->
      <div class="flex-1 min-h-14 overflow-y-auto rounded-md border border-default bg-muted/40 px-2 py-1">
        <p
          v-for="(m, i) in messages"
          :key="i"
          class="text-[10px] font-mono leading-4"
          :class="m.ok ? 'text-muted' : 'text-error'"
        >{{ m.text }}</p>
      </div>

    </div>

    <!-- Settings — full-cover overlay (window is small; a half slideover
         leaves an ugly strip of the main view showing). -->
    <Transition
      enter-active-class="transition-transform duration-200 ease-out"
      enter-from-class="translate-x-full"
      leave-active-class="transition-transform duration-200 ease-in"
      leave-to-class="translate-x-full"
    >
      <div
        v-if="settingsOpen"
        class="absolute inset-0 z-50 bg-default flex flex-col"
      >
        <!-- Header -->
        <div class="flex items-center gap-2 px-3 h-10 border-b border-default shrink-0">
          <UButton
            icon="i-lucide-arrow-left" color="neutral" variant="ghost" size="xs"
            :aria-label="t('close')" @click="settingsOpen = false"
          />
          <span class="text-sm font-semibold text-highlighted">{{ t('settings') }}</span>
        </div>

        <!-- Body -->
        <div class="flex-1 overflow-y-auto p-3 text-sm">
          <div
            v-if="settingsNotice"
            class="mb-3 rounded-md border px-2 py-1.5 text-[11px] leading-snug"
            :class="settingsNotice.ok ? 'border-primary/30 bg-primary/10 text-primary' : 'border-error/30 bg-error/10 text-error'"
          >
            {{ settingsNotice.text }}
          </div>

          <!-- Tabs -->
          <div class="flex gap-1 mb-3 bg-elevated/50 rounded-md p-0.5">
            <button
              class="flex-1 text-xs py-1 rounded transition-colors"
              :class="settingsTab === 'general' ? 'bg-default font-medium text-highlighted shadow-sm' : 'text-muted'"
              @click="settingsTab = 'general'"
            >{{ t('tabGeneral') }}</button>
            <button
              class="flex-1 text-xs py-1 rounded transition-colors"
              :class="settingsTab === 'services' ? 'bg-default font-medium text-highlighted shadow-sm' : 'text-muted'"
              @click="settingsTab = 'services'"
            >{{ t('tabServices') }}</button>
            <button
              class="flex-1 text-xs py-1 rounded transition-colors"
              :class="settingsTab === 'php' ? 'bg-default font-medium text-highlighted shadow-sm' : 'text-muted'"
              @click="settingsTab = 'php'"
            >{{ t('tabPhp') }}</button>
          </div>

          <!-- General tab -->
          <div v-show="settingsTab === 'general'" class="space-y-2.5">
            <div class="bg-elevated/40 rounded-md p-2">
              <p class="text-[11px] font-semibold text-highlighted mb-2">{{ t('appearance') }}</p>
              <div class="flex items-center justify-between mb-2">
                <span class="text-xs text-muted">{{ t('theme') }}</span>
                <div class="flex gap-0.5 bg-elevated/60 rounded-md p-0.5">
                  <button
                    v-for="opt in themeOptions" :key="opt.value"
                    class="text-[11px] px-2 py-0.5 rounded transition-colors"
                    :class="theme === opt.value ? 'bg-default font-medium text-highlighted shadow-sm' : 'text-muted'"
                    @click="theme = opt.value as 'system' | 'light' | 'dark'"
                  >{{ opt.label }}</button>
                </div>
              </div>
              <div class="flex items-center justify-between">
                <span class="text-xs text-muted">{{ t('language') }}</span>
                <button
                  class="flex items-center gap-1.5 px-1.5 py-0.5 rounded border border-default hover:bg-elevated/60 transition-colors"
                  @click="toggleLocale"
                >
                  <FlagIcon :locale="locale" />
                  <span class="text-[11px] font-medium uppercase">{{ locale }}</span>
                </button>
              </div>
            </div>

            <div class="bg-elevated/40 rounded-md p-2">
              <p class="text-[11px] font-semibold text-highlighted mb-2">{{ t('startup') }}</p>
              <div class="flex items-center justify-between mb-2">
                <span class="text-xs text-muted">{{ t('runAtStartup') }}</span>
                <USwitch v-model="runAtStartup" size="sm" />
              </div>
              <div class="flex items-center justify-between">
                <span class="text-xs text-muted">{{ t('startMinimized') }}</span>
                <USwitch v-model="startMinimized" size="sm" />
              </div>
            </div>

            <div class="bg-elevated/40 rounded-md p-2">
              <p class="text-[11px] font-semibold text-highlighted mb-2">{{ t('statusPolling') }}</p>
              <div class="flex items-center justify-between mb-2">
                <span class="text-xs text-muted">{{ t('autoRefresh') }}</span>
                <USwitch v-model="autoRefresh" size="sm" />
              </div>
              <div class="flex items-center justify-between" :class="!autoRefresh && 'opacity-40'">
                <span class="text-xs text-muted">{{ t('everyNSeconds', { n: refreshSec }) }}</span>
                <UInputNumber
                  v-model="refreshSec" :disabled="!autoRefresh"
                  :min="1" :max="60" size="xs" class="w-24"
                  :format-options="{ useGrouping: false }"
                />
              </div>
            </div>

            <div class="bg-elevated/40 rounded-md p-2">
              <p class="text-[11px] font-semibold text-highlighted mb-2">{{ t('webRoot') }}</p>
              <UInput v-model="docRoot" size="xs" class="w-full" placeholder="sites/default" />
              <p class="text-[10px] text-muted mt-1 leading-snug">
                {{ t('webRootHint') }}
              </p>
            </div>
          </div>

          <!-- PHP tab -->
          <div v-show="settingsTab === 'php'" class="space-y-2.5">
            <div class="bg-elevated/40 rounded-md p-2">
              <p class="text-[11px] font-semibold text-highlighted mb-2">{{ t('phpSettings') }}</p>
              <div class="space-y-1.5">
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted">{{ t('phpVersion') }}</span>
                  <UInput v-model="phpVersion" size="xs" class="w-24" :disabled="true" placeholder="8.4.21 (bundled)" />
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted">{{ t('phpCgiPort') }}</span>
                  <UInputNumber v-model="phpCgiPort" :min="1" :max="65535" size="xs" class="w-24" :format-options="{ useGrouping: false }" />
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted">{{ t('openPhpIni') }}</span>
                  <UButton @click="openTarget('phpIni', t('openPhpIni'))" color="neutral" variant="outline" size="xs">
                    {{ t('openPhpIni') }}
                  </UButton>
                </div>
                <div class="flex items-center justify-between mt-4">
                  <span class="text-xs text-muted italic">{{ t('phpHint') }}</span>
                </div>
              </div>
            </div>
          </div>

          <!-- Services tab -->
          <div v-show="settingsTab === 'services'" class="space-y-2.5">
            <div class="bg-elevated/40 rounded-md p-2">
              <p class="text-[11px] font-semibold text-highlighted mb-2">{{ t('ports') }}</p>
              <div class="space-y-1.5">
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted">{{ t('nginxWeb') }}</span>
                  <UInputNumber v-model="portWeb" :min="1" :max="65535" size="xs" class="w-24" :format-options="{ useGrouping: false }" />
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted">{{ t('mariadb') }}</span>
                  <UInputNumber v-model="portMysql" :min="1" :max="65535" size="xs" class="w-24" :format-options="{ useGrouping: false }" />
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted">{{ t('redis') }}</span>
                  <UInputNumber v-model="portRedis" :min="1" :max="65535" size="xs" class="w-24" :format-options="{ useGrouping: false }" />
                </div>
              </div>
            </div>

            <div class="bg-elevated/40 rounded-md p-2">
              <p class="text-[11px] font-semibold text-highlighted mb-2">{{ t('startOnLaunch') }}</p>
              <div class="space-y-1.5">
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted">{{ t('nginx') }}</span>
                  <USwitch v-model="autostartKinds.nginx" size="sm" />
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted">{{ t('mariadb') }}</span>
                  <USwitch v-model="autostartKinds.mysql" size="sm" />
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted">{{ t('redis') }}</span>
                  <USwitch v-model="autostartKinds.redis" size="sm" />
                </div>
              </div>
            </div>

            <UButton
              icon="i-lucide-file-cog" color="neutral" variant="outline"
              size="xs" block @click="openTarget('config', t('openConfigDir'))"
            >{{ t('openConfigDir') }}</UButton>
          </div>
        </div>

        <!-- Footer -->
        <div class="px-3 h-11 flex items-center gap-2 border-t border-default shrink-0">
          <button
            class="text-[10px] font-mono text-muted flex items-center gap-1 hover:text-highlighted transition-colors"
            :title="t('sourceCode')"
            @click="openTarget('repo', t('sourceCode'))"
          >
            <UIcon name="i-lucide-github" class="size-3" />
            <span class="truncate">SushiBox · Inari v0.1.0</span>
          </button>
          <span class="flex-1" />
          <UButton color="neutral" variant="ghost" size="xs" @click="settingsOpen = false">
            {{ t('close') }}
          </UButton>
          <UButton color="primary" size="xs" :loading="savingSettings" @click="saveServerSettings">
            {{ t('save') }}
          </UButton>
        </div>
      </div>
    </Transition>
  </div>
</template>
