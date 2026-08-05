package edu.crabmate

import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback

class MainActivity : TauriActivity() {
  /** 与 Tauri Android 默认资产源一致（`useHttpsScheme=false` → http）。 */
  private var connectHomeUrl: String = "http://tauri.localhost/"
  private var appWebView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    // 不要 enableEdgeToEdge()：Android WebView 通常不提供 CSS safe-area-inset-*，
    // 铺满状态栏后会与远程 Web 顶栏按钮重叠、无法点击。
    super.onCreate(savedInstanceState)

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

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    appWebView = webView
    webView.addJavascriptInterface(MobileBridge(), "CrabMateMobile")
    webView.post { rememberConnectHomeIfAppOrigin(webView.url) }
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
    view.loadUrl(connectHomeUrl.ifBlank { "http://tauri.localhost/" })
  }

  /** 供远程 Web（无 Tauri IPC）调用：断开并回连接页。 */
  inner class MobileBridge {
    @JavascriptInterface
    fun disconnect() {
      runOnUiThread { loadConnectPage() }
    }

    @JavascriptInterface
    fun isRemoteClient(): Boolean = true
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
