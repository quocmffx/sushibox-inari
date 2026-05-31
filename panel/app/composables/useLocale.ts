// Lightweight i18n — no heavy dependency. en/vi, persisted in localStorage.

export type Locale = 'en' | 'vi'

type Dict = Record<string, string>

const messages: Record<Locale, Dict> = {
  en: {
    // Runtime card
    runtime:        'Runtime',
    start:          'Start',
    stop:           'Stop',
    restart:        'Restart',
    startAll:       'Start all',
    stopAll:        'Stop all',
    restartAll:     'Restart all',
    running:        'Running',
    stopped:        'Stopped',
    openSite:       'Open site',
    siteFolder:     'Site folder',
    logsFolder:     'Logs',

    // Header / misc
    cannotReachApi: 'Cannot reach the backend',
    refresh:        'Refresh',
    settings:       'Settings',
    lightMode:      'Light mode',
    darkMode:       'Dark mode',
    ready:          'Ready.',

    // Settings — tabs
    tabGeneral:     'General',
    tabServices:    'Services',

    // Settings — general
    appearance:     'Appearance',
    theme:          'Theme',
    themeSystem:    'System',
    themeLight:     'Light',
    themeDark:      'Dark',
    statusPolling:  'Status polling',
    autoRefresh:    'Auto refresh',
    everyNSeconds:  'Every {n}s',
    webRoot:        'Web root',
    webRootHint:    'Relative to the app folder. Restart Nginx to apply.',
    language:       'Language',
    startup:        'Startup',
    runAtStartup:   'Run at Windows startup',
    startMinimized: 'Start minimized to tray',

    // Settings — services
    ports:          'Ports',
    nginxWeb:       'Nginx (web)',
    mariadb:        'MariaDB',
    redis:          'Redis',
    startOnLaunch:  'Start on launch',
    nginx:          'Nginx',
    phpCgi:         'PHP-CGI',
    openConfigDir:  'Open config folder',

    // Settings — footer
    close:          'Close',
    save:           'Save',
    sourceCode:     'Source code',

    // Activity messages
    settingsSaved:  'Settings saved. Restart affected services to apply ports.',
    saveFailed:     'Save failed',
    actionFailed:   'Action failed',
    requestFailed:  'Request failed',
    opened:         'Opened {label}',
  },
  vi: {
    runtime:        'Dịch vụ',
    start:          'Chạy',
    stop:           'Dừng',
    restart:        'Khởi động lại',
    startAll:       'Chạy tất cả',
    stopAll:        'Dừng tất cả',
    restartAll:     'Khởi động lại tất cả',
    running:        'Đang chạy',
    stopped:        'Đã dừng',
    openSite:       'Mở trang web',
    siteFolder:     'Thư mục web',
    logsFolder:     'Nhật ký',

    cannotReachApi: 'Không kết nối được backend',
    refresh:        'Làm mới',
    settings:       'Cài đặt',
    lightMode:      'Chế độ sáng',
    darkMode:       'Chế độ tối',
    ready:          'Sẵn sàng.',

    tabGeneral:     'Chung',
    tabServices:    'Dịch vụ',

    appearance:     'Giao diện',
    theme:          'Chủ đề',
    themeSystem:    'Hệ thống',
    themeLight:     'Sáng',
    themeDark:      'Tối',
    statusPolling:  'Tự cập nhật trạng thái',
    autoRefresh:    'Tự làm mới',
    everyNSeconds:  'Mỗi {n} giây',
    webRoot:        'Thư mục web',
    webRootHint:    'Tính từ thư mục ứng dụng. Khởi động lại Nginx để áp dụng.',
    language:       'Ngôn ngữ',
    startup:        'Khởi động',
    runAtStartup:   'Chạy cùng Windows',
    startMinimized: 'Mở thu nhỏ xuống tray',

    ports:          'Cổng',
    nginxWeb:       'Nginx (web)',
    mariadb:        'MariaDB',
    redis:          'Redis',
    startOnLaunch:  'Chạy khi mở app',
    nginx:          'Nginx',
    phpCgi:         'PHP-CGI',
    openConfigDir:  'Mở thư mục cấu hình',

    close:          'Đóng',
    save:           'Lưu',
    sourceCode:     'Mã nguồn',

    settingsSaved:  'Đã lưu. Khởi động lại dịch vụ liên quan để áp dụng cổng.',
    saveFailed:     'Lưu thất bại',
    actionFailed:   'Thao tác thất bại',
    requestFailed:  'Yêu cầu thất bại',
    opened:         'Đã mở {label}',
  },
}

const STORAGE_KEY = 'inari.locale'

// Module-level singleton so every component shares the same reactive locale.
const locale = ref<Locale>('en')

export function useLocale() {
  // Hydrate from localStorage on first client use.
  if (import.meta.client && !locale.value) locale.value = 'en'

  function load() {
    if (!import.meta.client) return
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved === 'en' || saved === 'vi') locale.value = saved
  }

  function setLocale(l: Locale) {
    locale.value = l
    if (import.meta.client) localStorage.setItem(STORAGE_KEY, l)
  }

  function toggle() {
    setLocale(locale.value === 'en' ? 'vi' : 'en')
  }

  // t('key', { n: 5 }) → interpolates {n}
  function t(key: string, vars?: Record<string, string | number>): string {
    let s = messages[locale.value][key] ?? messages.en[key] ?? key
    if (vars) for (const [k, v] of Object.entries(vars)) s = s.replace(`{${k}}`, String(v))
    return s
  }

  return { locale, setLocale, toggle, load, t }
}
