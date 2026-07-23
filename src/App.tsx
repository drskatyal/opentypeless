import { useEffect, useState } from 'react'
import i18n from './i18n'
import { useTauriEvents } from './hooks/useTauriEvents'
import { useTheme } from './hooks/useTheme'
import { useAppStore } from './stores/appStore'
import { useAuthStore } from './stores/authStore'
import { useRoute } from './lib/router'
import {
  loadOnboardingCompleted,
  getConfig,
  getHistory,
  getDictionary,
  getCorrectionRules,
  checkAccessibilityPermission,
  getPlatformCapabilities,
  getHotkeyRegistrationError,
} from './lib/tauri'
import { initDeepLinkListener } from './lib/deep-link'
import { Capsule } from './components/Capsule'
import { Settings } from './components/Settings'
import { History } from './components/History'
import { Onboarding } from './components/Onboarding'
import { MainLayout } from './components/MainLayout'
import { HomePage } from './components/HomePage'
import { UpgradePage } from './components/UpgradePage'
import { AccountPage } from './components/AccountPage'
import { AskPanel } from './components/AskPanel'
import { FloatingEditor } from './components/FloatingEditor'
import { ToastContainer } from './components/Toast'
import { ActHud } from './components/ActHud'
import { FloatingAgents } from './components/AgentsBoard/FloatingAgents'

function CapsuleApp() {
  useTauriEvents()
  useTheme()

  const setConfig = useAppStore((s) => s.setConfig)

  useEffect(() => {
    // Load config so DurationTimer gets the correct max_recording_seconds
    getConfig()
      .then((config) => {
        setConfig(config)
        // Restore UI language from config
        if (config.ui_language && config.ui_language !== i18n.language) {
          i18n.changeLanguage(config.ui_language)
          localStorage.setItem('ui_language', config.ui_language)
        }
      })
      .catch((e) => {
        console.error('Failed to load config in capsule:', e)
      })
  }, [setConfig])

  // Window show is handled by useCapsuleResize (setSize → setPosition → show),
  // which works on both Windows and macOS. The previous rAF-based show approach
  // failed on macOS because WKWebView pauses requestAnimationFrame in hidden windows.
  return <Capsule />
}

function AskApp() {
  useTheme()
  const setConfig = useAppStore((s) => s.setConfig)

  useEffect(() => {
    useAuthStore.getState().initialize()
    getConfig()
      .then((config) => {
        setConfig(config)
        if (config.ui_language && config.ui_language !== i18n.language) {
          i18n.changeLanguage(config.ui_language)
          localStorage.setItem('ui_language', config.ui_language)
        }
      })
      .catch((e) => {
        console.error('Failed to load config in Ask app:', e)
      })
  }, [setConfig])

  return (
    <>
      <AskPanel />
      <ToastContainer />
    </>
  )
}

function EditorApp() {
  useTheme()
  return <FloatingEditor />
}

function AgentsApp() {
  useTheme()
  const setConfig = useAppStore((s) => s.setConfig)

  useEffect(() => {
    // The floating agents window needs config for `act_enabled` (whether to show
    // at all) and to react live when Act is toggled on/off.
    getConfig()
      .then(setConfig)
      .catch((e) => console.error('Failed to load config in agents window:', e))
  }, [setConfig])

  return <FloatingAgents />
}

function ActHudApp() {
  useTheme()
  const setConfig = useAppStore((s) => s.setConfig)

  useEffect(() => {
    // The Act HUD overlay needs config for `act_enabled` — it drives whether the
    // window is shown at all, and reacts live when Act is toggled on/off.
    getConfig()
      .then(setConfig)
      .catch((e) => console.error('Failed to load config in acthud window:', e))
  }, [setConfig])

  return <ActHud />
}

function MainApp() {
  useTauriEvents()
  useTheme()

  const onboardingCompleted = useAppStore((s) => s.onboardingCompleted)
  const setOnboardingCompleted = useAppStore((s) => s.setOnboardingCompleted)
  const setConfig = useAppStore((s) => s.setConfig)
  const setSavedConfig = useAppStore((s) => s.setSavedConfig)
  const setHistory = useAppStore((s) => s.setHistory)
  const setDictionary = useAppStore((s) => s.setDictionary)
  const setCorrectionRules = useAppStore((s) => s.setCorrectionRules)
  const setAccessibilityTrusted = useAppStore((s) => s.setAccessibilityTrusted)
  const setPlatformCapabilities = useAppStore((s) => s.setPlatformCapabilities)
  const setHotkeyRegistrationError = useAppStore((s) => s.setHotkeyRegistrationError)
  const [loaded, setLoaded] = useState(false)
  const [loadError, setLoadError] = useState(false)
  const { route } = useRoute()

  useEffect(() => {
    let cancelled = false

    // The initial load fires six backend commands at once via Promise.all
    // (all-or-nothing). On a cold start the webview can mount and call these
    // BEFORE the Tauri backend has finished its setup — so a single not-yet-ready
    // command rejected the whole batch and dropped the user on the "Failed to load
    // application data" screen, which a manual Retry (full reload) then fixed
    // because the backend was up by then. Retry the batch a few times with a short
    // backoff so that transient cold-start race self-heals silently; only surface
    // the error screen if every attempt fails.
    const loadInitialData = async () => {
      const MAX_ATTEMPTS = 6
      for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
        try {
          const [
            config,
            history,
            dictionary,
            correctionRules,
            platformCapabilities,
            hotkeyRegistrationError,
          ] = await Promise.all([
            getConfig(),
            getHistory(200, 0),
            getDictionary(),
            getCorrectionRules(),
            getPlatformCapabilities(),
            getHotkeyRegistrationError(),
          ])
          if (cancelled) return
          setConfig(config)
          setSavedConfig(config)
          setHistory(history)
          setDictionary(dictionary)
          setCorrectionRules(correctionRules)
          setPlatformCapabilities(platformCapabilities)
          setHotkeyRegistrationError(hotkeyRegistrationError)
          // Check macOS Accessibility permission
          if (navigator.platform.toUpperCase().indexOf('MAC') >= 0) {
            checkAccessibilityPermission().then((trusted) => {
              setAccessibilityTrusted(trusted)
            })
          }
          // Restore UI language from config
          if (config.ui_language && config.ui_language !== i18n.language) {
            i18n.changeLanguage(config.ui_language)
            localStorage.setItem('ui_language', config.ui_language)
          }
          return
        } catch (e) {
          if (cancelled) return
          console.error(`Failed to load initial data (attempt ${attempt}/${MAX_ATTEMPTS}):`, e)
          if (attempt === MAX_ATTEMPTS) {
            setLoadError(true)
            return
          }
          // Linear backoff (250ms, 500ms, …) — the backend is usually ready
          // within the first retry, so this stays invisibly fast in practice.
          await new Promise((resolve) => setTimeout(resolve, 250 * attempt))
        }
      }
    }

    loadOnboardingCompleted().then(async (done) => {
      if (cancelled) return
      setOnboardingCompleted(done)
      if (done) {
        await loadInitialData()
      }
      if (!cancelled) setLoaded(true)
    })

    // Initialize auth session (non-blocking)
    useAuthStore.getState().initialize()

    // Initialize deep-link listener
    initDeepLinkListener()

    return () => {
      cancelled = true
    }
  }, [
    setOnboardingCompleted,
    setConfig,
    setSavedConfig,
    setHistory,
    setDictionary,
    setCorrectionRules,
    setAccessibilityTrusted,
    setPlatformCapabilities,
    setHotkeyRegistrationError,
  ])

  const user = useAuthStore((s) => s.user)

  // Periodically refresh subscription status + refresh on window focus (throttled)
  useEffect(() => {
    if (!loaded || !user) return

    let lastRefresh = 0
    const throttledRefresh = () => {
      const now = Date.now()
      const { checkoutPending } = useAuthStore.getState()
      // Skip throttle if user just came back from checkout
      if (!checkoutPending && now - lastRefresh < 30_000) return
      lastRefresh = now
      useAuthStore.getState().refreshSubscription()
    }

    const interval = setInterval(
      () => {
        lastRefresh = Date.now()
        useAuthStore.getState().refreshSubscription()
      },
      5 * 60 * 1000,
    )

    window.addEventListener('focus', throttledRefresh)

    return () => {
      clearInterval(interval)
      window.removeEventListener('focus', throttledRefresh)
    }
  }, [loaded, user])

  if (!loaded)
    return (
      <div className="flex items-center justify-center h-screen">
        <span className="text-text-tertiary text-[13px]">Loading...</span>
      </div>
    )
  if (loadError)
    return (
      <div className="flex flex-col items-center justify-center h-screen gap-3">
        <span className="text-error text-[13px]">Failed to load application data.</span>
        <button
          onClick={() => window.location.reload()}
          className="px-4 py-2 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover transition-colors"
        >
          Retry
        </button>
      </div>
    )
  if (!onboardingCompleted) return <Onboarding />

  return (
    <MainLayout>
      {route === 'home' && <HomePage />}
      {route === 'settings' && <Settings />}
      {route === 'history' && <History />}
      {route === 'upgrade' && <UpgradePage />}
      {route === 'account' && <AccountPage />}
      <ToastContainer />
    </MainLayout>
  )
}

function App() {
  // Capsule window loads with #capsule hash — detect synchronously, no race condition
  if (window.location.hash === '#capsule') return <CapsuleApp />
  if (window.location.hash === '#ask') return <AskApp />
  if (window.location.hash === '#editor') return <EditorApp />
  if (window.location.hash === '#agents') return <AgentsApp />
  if (window.location.hash === '#acthud') return <ActHudApp />
  return <MainApp />
}

export default App
