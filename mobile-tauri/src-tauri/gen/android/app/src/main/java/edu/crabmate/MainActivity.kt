package edu.crabmate

import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.autofill.AutofillManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import kotlin.math.roundToInt

class MainActivity : TauriActivity() {
  /** 与 Tauri Android 默认资产源一致（`useHttpsScheme=false` → http）。 */
  private var connectHomeUrl: String = "http://tauri.localhost/"
  private var appWebView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    // 不要 enableEdgeToEdge()：Android WebView 通常不提供 CSS safe-area-inset-*，
    // 铺满状态栏后会与远程 Web 顶栏按钮重叠、无法点击。
    WindowCompat.setDecorFitsSystemWindows(window, true)
    super.onCreate(savedInstanceState)
    // Tauri / Activity 基类可能在 super 里改回 edge-to-edge，再强制一次。
    WindowCompat.setDecorFitsSystemWindows(window, true)

    onBackPressedDispatcher.addCallback(
      this,
      object : OnBackPressedCallback(true) {
        override fun handleOnBackPressed() {
          val url = appWebView?.url
          if (isAppOrigin(url)) {
            // 已在连接页：退出 App
            isEnabled = false
            onBackPressedDispatcher.onBackPressed()
            isEnabled = true
          } else {
            // 远程 UI：回到本地连接页（可改服务器 / Bearer）
            loadConnectPage()
          }
        }
      },
    )
  }

  override fun onStart() {
    super.onStart()
    WindowCompat.setDecorFitsSystemWindows(window, true)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    appWebView = webView
    WindowCompat.setDecorFitsSystemWindows(window, true)
    // 允许系统 Autofill / 密码管理器填充连接页的 URL+Bearer
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      webView.importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_YES
    }
    webView.addJavascriptInterface(MobileBridge(), "CrabMateMobile")
    // 页面加载后注入上下安全区 CSS 变量
    webView.post { injectSafeInsetsCss(webView) }
    ViewCompat.setOnApplyWindowInsetsListener(webView) { v, insets ->
      injectSafeInsetsCss(v as? WebView ?: webView)
      insets
    }
    webView.post { rememberConnectHomeIfAppOrigin(webView.url) }
  }

  private fun injectSafeInsetsCss(webView: WebView) {
    val topPx = statusBarInsetCssPx()
    val bottomPx = navBarInsetCssPx()
    val js =
      "(function(){try{var r=document.documentElement;" +
        "r.style.setProperty('--cm-safe-top','${topPx}px');" +
        "r.style.setProperty('--cm-safe-bottom','${bottomPx}px');" +
        "r.setAttribute('data-cm-mobile-shell','');}catch(e){}})();"
    webView.evaluateJavascript(js, null)
  }

  private fun rememberConnectHomeIfAppOrigin(url: String?) {
    if (url.isNullOrBlank() || !isAppOrigin(url)) {
      return
    }
    connectHomeUrl = stripFragmentAndQuery(url).ifBlank { "http://tauri.localhost/" }
  }

  private fun loadConnectPage() {
    val view = appWebView ?: return
    rememberConnectHomeIfAppOrigin(view.url)
    val base = connectHomeUrl.ifBlank { "http://tauri.localhost/" }
    // ?manual=1：跳过连接页冷启动自动登录，便于更换服务器
    val sep = if (base.contains('?')) '&' else '?'
    view.loadUrl("$base${sep}manual=1")
  }

  private fun autofillManager(): AutofillManager? {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
      return null
    }
    return getSystemService(AutofillManager::class.java)
  }

  /** 顶栏安全区（CSS px）：状态栏/刘海高度 + 少量触控余量，供 Web `--cm-safe-top`。 */
  private fun statusBarInsetCssPx(): Int {
    val density = resources.displayMetrics.density.coerceAtLeast(0.5f)
    var topPx = 0
    val types = WindowInsetsCompat.Type.statusBars() or WindowInsetsCompat.Type.displayCutout()
    ViewCompat.getRootWindowInsets(window.decorView)?.let { insets ->
      topPx = insets.getInsetsIgnoringVisibility(types).top
    }
    if (topPx <= 0) {
      val id = resources.getIdentifier("status_bar_height", "dimen", "android")
      if (id > 0) {
        topPx = resources.getDimensionPixelSize(id)
      }
    }
    val css = (topPx / density).roundToInt()
    // 已 setDecorFitsSystemWindows(true) 时 WebView 多数已避开状态栏；仅保留小余量与保底，
    // 避免再叠一层过大 padding（旧：+28 且至少 52）。
    return (css + 4).coerceAtLeast(24)
  }

  /** 系统导航栏/手势条高度（CSS px），供底栏状态条避开。 */
  private fun navBarInsetCssPx(): Int {
    val density = resources.displayMetrics.density.coerceAtLeast(0.5f)
    var bottomPx = 0
    ViewCompat.getRootWindowInsets(window.decorView)?.let { insets ->
      bottomPx = insets.getInsetsIgnoringVisibility(WindowInsetsCompat.Type.navigationBars()).bottom
    }
    val css = (bottomPx / density).roundToInt()
    return (css + 8).coerceAtLeast(24)
  }

  /** 供连接页 / 远程 Web 调用。 */
  inner class MobileBridge {
    @JavascriptInterface
    fun disconnect() {
      runOnUiThread { loadConnectPage() }
    }

    @JavascriptInterface
    fun isRemoteClient(): Boolean = true

    /** 顶栏安全区（CSS 像素）。 */
    @JavascriptInterface
    fun getStatusBarInsetPx(): Int = statusBarInsetCssPx()

    /** 底栏 / 系统导航安全区（CSS 像素）。 */
    @JavascriptInterface
    fun getNavBarInsetPx(): Int = navBarInsetCssPx()

    /** 连接探测成功后调用，提示系统密码管理器保存 URL+Bearer。 */
    @JavascriptInterface
    fun notifyLoginSuccess() {
      runOnUiThread {
        autofillManager()?.commit()
      }
    }

    /** 连接失败时取消本次 Autofill 会话。 */
    @JavascriptInterface
    fun notifyLoginFailure() {
      runOnUiThread {
        autofillManager()?.cancel()
      }
    }
    /** 在系统浏览器中打开 http(s)/mailto（远程 WebView 内 `window.open` 通常无效）。 */
    @JavascriptInterface
    fun openExternalUrl(url: String) {
      runOnUiThread {
        try {
          val uri = android.net.Uri.parse(url.trim())
          val scheme = uri.scheme?.lowercase()
          if (scheme != "http" && scheme != "https" && scheme != "mailto") {
            return@runOnUiThread
          }
          startActivity(android.content.Intent(android.content.Intent.ACTION_VIEW, uri))
        } catch (_: Exception) {
          // 无浏览器或非法 URL：静默忽略
        }
      }
    }
  }

  companion object {
    fun isAppOrigin(url: String?): Boolean {
      if (url.isNullOrBlank()) {
        return true
      }
      val u = url.lowercase()
      return u.startsWith("tauri://") ||
        u.startsWith("asset://") ||
        u.contains("://tauri.localhost")
    }

    fun stripFragmentAndQuery(url: String): String {
      val noFrag = url.substringBefore('#')
      return noFrag.substringBefore('?')
    }
  }
}
