package com.asmrplayer.android.sync

import android.annotation.SuppressLint
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import com.asmrplayer.android.security.TokenCipher
import org.json.JSONObject
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit

/** A private WebView runs the bundled official SDK for both UI and WorkManager.
 * Only the local SDK page gets a reply bridge. No password or session is saved by the WebView. */
internal class SupabaseProvider(context: Context) : SyncProvider {
    private val app = context.applicationContext
    private val prefs = app.getSharedPreferences("supabase-sync",Context.MODE_PRIVATE)
    private val cipher = TokenCipher("asmr-supabase-refresh-v1")
    private val main = Handler(Looper.getMainLooper())

    private class Reply(private val result: ArrayBlockingQueue<String>) {
        @JavascriptInterface fun finish(json: String) { result.offer(json) }
    }
    @SuppressLint("SetJavaScriptEnabled")
    private fun request(input: JSONObject): JSONObject {
        check(Looper.myLooper() != Looper.getMainLooper())
        val result=ArrayBlockingQueue<String>(1)
        var view: WebView? = null
        main.post {
            try {
                val web=WebView(app)
                view=web
                web.settings.javaScriptEnabled=true
                web.settings.allowFileAccess=false
                web.settings.allowContentAccess=false
                web.addJavascriptInterface(Reply(result),"NativeSync")
                web.webViewClient=object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(v: WebView, req: WebResourceRequest) = true
                    @Deprecated("Legacy WebView navigation")
                    override fun shouldOverrideUrlLoading(v: WebView, url:String) = true
                    override fun shouldInterceptRequest(v: WebView, req: WebResourceRequest): WebResourceResponse? {
                        if (req.url.toString()=="https://appassets.androidplatform.net/sdk.js") {
                            return WebResourceResponse("application/javascript","utf-8",app.assets.open("supabase-sync.js"))
                        }
                        return super.shouldInterceptRequest(v,req)
                    }
                    override fun onPageFinished(v:WebView,url:String) {
                        if(url!="https://appassets.androidplatform.net/") return
                        val data=JSONObject.quote(input.toString())
                        v.evaluateJavascript("""(async()=>{try{const data=await AsmrSync.execute(JSON.parse($data));NativeSync.finish(JSON.stringify({data}));}catch(e){NativeSync.finish(JSON.stringify({error:e.message||'Supabase 请求失败'}));}})()""",null)
                    }
                }
                web.loadDataWithBaseURL("https://appassets.androidplatform.net/",
                    """<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self'; connect-src https://iacwfcnrhnsddesdqjlz.supabase.co"><script src="/sdk.js"></script>""",
                    "text/html","utf-8",null)
            } catch (e:Exception) { result.offer(JSONObject().put("error",e.message ?: "无法启动登录服务").toString()) }
        }
        try {
            val reply=JSONObject(result.poll(60,TimeUnit.SECONDS) ?: error("Supabase 请求超时"))
            if(reply.has("error")) error(reply.getString("error"))
            return reply.getJSONObject("data")
        } finally {
            main.post { view?.apply { removeJavascriptInterface("NativeSync"); destroy() } }
        }
    }
    private fun save(session: JSONObject):String {
        val encrypted=cipher.encrypt(session.getString("refreshToken"),"supabase")
        check(prefs.edit().putString("refresh",encrypted).commit()) { "无法保存 Supabase 登录" }
        session.remove("refreshToken")
        return session.toString()
    }
    override fun login(input:LoginInput):String {
        val credentials=input as LoginInput.Email
        return save(request(JSONObject().put("action",if(credentials.signup) "signup" else "login")
            .put("email",credentials.email).put("password",credentials.password)))
    }
    override fun getStatus()=BindingStatus(true,prefs.contains("refresh"))
    override fun logout(){ check(prefs.edit().clear().commit()) }
    override fun accessToken():String {
        val encrypted=requireNotNull(prefs.getString("refresh",null)) { "请先登录 Supabase" }
        return save(request(JSONObject().put("action","refresh").put("refreshToken",cipher.decrypt(encrypted,"supabase"))))
    }
    override fun downloadSyncData(token:String,device:String,merged:JSONObject):String? {
        var cursor=""
        while(true) {
            val row=request(JSONObject().put("action","select").put("session",JSONObject(token)).put("after",cursor))
            if(row.length()==0) return null
            val next=row.getString("device_id")
            check(next>cursor) { "云端设备分页无效" }
            val doc=row.getJSONObject("sync_data")
            require(doc.toString().toByteArray().size<=SyncProtocol.LIMIT)
            SyncProtocol.merge(merged,doc)
            cursor=next
        }
    }
    override fun uploadSyncData(token:String,device:String,merged:JSONObject,own:String?) {
        require(merged.toString().toByteArray().size<=SyncProtocol.LIMIT)
        request(JSONObject().put("action","upsert").put("session",JSONObject(token)).put("device",device).put("document",SyncProtocol.wire(merged)))
    }
}
