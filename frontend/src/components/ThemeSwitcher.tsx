import { useState, useEffect } from 'react'
import { Palette } from 'lucide-react'

const THEME_KEY = 'agent-demo-theme'

const THEMES = [
  { id: 'night', name: '夜间' },
  { id: 'light', name: '浅色' },
  { id: 'dark', name: '深色' },
  { id: 'cupcake', name: '纸杯' },
  { id: 'synthwave', name: '霓虹' },
  { id: 'forest', name: '森林' },
] as const

type ThemeId = (typeof THEMES)[number]['id']

function getStoredTheme(): ThemeId {
  if (typeof window === 'undefined') return 'light'
  const stored = localStorage.getItem(THEME_KEY) as ThemeId | null
  return THEMES.some((t) => t.id === stored) ? stored! : 'light'
}

function applyTheme(themeId: ThemeId) {
  document.documentElement.setAttribute('data-theme', themeId)
  localStorage.setItem(THEME_KEY, themeId)
}

export function ThemeSwitcher() {
  const [theme, setTheme] = useState<ThemeId>(() => getStoredTheme())

  useEffect(() => {
    applyTheme(theme)
  }, [theme])

  return (
    <div className="dropdown dropdown-end">
      <label tabIndex={0} className="btn btn-ghost btn-sm btn-circle" title="切换主题">
        <Palette size={18} />
      </label>
      <ul
        tabIndex={0}
        className="dropdown-content menu menu-sm z-50 mt-2 p-2 shadow-xl bg-base-200 border border-base-content/10 w-36 rounded-xl"
      >
        {THEMES.map((t) => (
          <li key={t.id}>
            <button
              type="button"
              className={theme === t.id ? 'active' : ''}
              onClick={() => setTheme(t.id)}
            >
              {t.name}
              {theme === t.id && <span className="text-primary"> ✓</span>}
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}
