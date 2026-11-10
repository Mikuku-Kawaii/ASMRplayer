package com.asmrplayer.android.ui

import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.text.input.PasswordVisualTransformation
import android.app.Activity
import net.openid.appauth.AuthorizationResponse
import net.openid.appauth.AuthorizationService
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.sync.*
import kotlinx.coroutines.*

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun SyncPage() {
    val context = LocalContext.current
    val repo = remember { AndroidBackend.get(context).sync }
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf<SyncStatus?>(null) }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf("") }
    var pendingProvider by rememberSaveable { mutableStateOf<Storage?>(null) }
    var davUrl by remember { mutableStateOf("") }
    var davUser by remember { mutableStateOf("") }
    var davPassword by remember { mutableStateOf("") }
    var email by remember { mutableStateOf("") }
    var emailPassword by remember { mutableStateOf("") }
    LaunchedEffect(status?.provider,status?.connected) { emailPassword="" }
    var githubCode by remember { mutableStateOf<GitHubAuthorization.Challenge?>(null) }
    var githubJob by remember { mutableStateOf<Job?>(null) }
    var useToken by remember { mutableStateOf(false) }
    var gistToken by remember { mutableStateOf("") }
    LaunchedEffect(status?.connected) { if (status?.connected == true) {gistToken = ""; davPassword = ""} }
    var browser by remember { mutableStateOf<AuthorizationService?>(null) }
    DisposableEffect(browser) { val service = browser; onDispose { service?.dispose() } }
    LaunchedEffect(busy) {
        if (!busy) while (true) {
            try { status = withContext(Dispatchers.IO) { repo.status() } }
            catch (e: CancellationException) { throw e }
            catch (e: Exception) { message = e.message ?: "无法读取同步状态" }
            delay(4000)
        }
    }
    fun operate(block: () -> SyncStatus) {
        busy = true; message = ""
        scope.launch {
            try { status = withContext(Dispatchers.IO) { block() } }
            catch (e: CancellationException) { throw e }
            catch (e: Exception) { message = e.message ?: "操作失败，请重试" }
            finally { busy = false }
        }
    }
    val noticeHost = LocalNoticeHost.current
    LaunchedEffect(message) { if (message.isNotEmpty()) noticeHost?.show(com.asmrplayer.android.UiFeedback(message)) }
    val login = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        browser = null
        val id=pendingProvider
        pendingProvider=null
        val response = result.data?.let(AuthorizationResponse::fromIntent)
        if (result.resultCode == Activity.RESULT_OK && response != null && id != null) {
            operate { repo.syncAuthorized(id,LoginInput.Browser(response)) }
        } else {
            busy = false
            message = if (result.resultCode == Activity.RESULT_CANCELED) "已取消 OneDrive 登录" else "OneDrive 授权未成功，请重新登录"
        }
    }
    val id = status?.provider ?: Storage.GITHUB
    var showConflicts by rememberSaveable(id) { mutableStateOf(false) }
    LazyColumn(Modifier.fillMaxSize().navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item { MaintenanceCard("同步账号") {
            if (busy || (status == null && message.isEmpty())) LoadingRing()
            ChoiceMenu("存储方式",id,Storage.entries.map { it to it.title }) { next ->
            if(!busy && next!=id) { gistToken=""; davPassword=""; githubCode=null; useToken=false; operate {repo.select(next)} }
            }
            if(status?.connected!=true) MaintenanceHint("在两台设备上选择同一存储方式，并登录同一账号。")
            if (status?.configured == false) Text("此版本暂未启用${id.title}登录。")
            else if(status?.connected != true && id==Storage.SUPABASE) Column(verticalArrangement=Arrangement.spacedBy(8.dp)){
                OutlinedTextField(email,{email=it},singleLine=true,enabled=!busy,label={Text("邮箱")})
                OutlinedTextField(emailPassword,{emailPassword=it},singleLine=true,enabled=!busy,label={Text("密码")},visualTransformation=PasswordVisualTransformation())
                val enabled=!busy && email.isNotBlank() && emailPassword.isNotEmpty()
                MotionButton({val input=LoginInput.Email(email,emailPassword); operate { repo.syncAuthorized(id,input) }},enabled=enabled){Text(if(busy) "正在连接…" else "登录并同步")}
                MotionTextButton({val input=LoginInput.Email(email,emailPassword,true); operate { repo.syncAuthorized(id,input) }},enabled=enabled){Text("注册账号")}
            }
            else if(status?.connected != true && id==Storage.WEBDAV) Column(verticalArrangement=Arrangement.spacedBy(8.dp)){
                OutlinedTextField(davUrl,{davUrl=it},singleLine=true,enabled=!busy,label={Text("HTTPS 文件夹地址")})
                OutlinedTextField(davUser,{davUser=it},singleLine=true,enabled=!busy,label={Text("用户名")})
                OutlinedTextField(davPassword,{davPassword=it},singleLine=true,enabled=!busy,visualTransformation=PasswordVisualTransformation(),label={Text("密码或应用密码")})
                MotionButton({val input=LoginInput.WebDav(davUrl,davUser,davPassword);operate {repo.syncAuthorized(id,input)}},
                    enabled=!busy && davUrl.isNotBlank() && davUser.isNotBlank() && davPassword.isNotEmpty()){Text("连接 WebDAV 并同步")}
            }
            else if (status?.connected != true && id==Storage.GITHUB) Column(verticalArrangement=Arrangement.spacedBy(8.dp)){
                Text("知道秘密 Gist 链接的人可以读取同步内容。",style=MaterialTheme.typography.bodySmall)
                if(!useToken){
                    val challenge=githubCode
                    if(challenge==null) MotionButton({
                        busy=true;message=""
                        scope.launch {
                            try { githubCode=withContext(Dispatchers.IO){GitHubAuthorization.begin()} }
                            catch(e:CancellationException){throw e}
                            catch(e:Exception){message=e.message ?: "无法开始 GitHub 授权"}
                            finally{busy=false}
                        }
                    },enabled=!busy){Text("登录 GitHub 并同步")}
                    else {
                        Text("GitHub 授权码：${challenge.userCode}")
                        MotionButton({
                            busy=true;message=""
                            githubJob=scope.launch {
                                try {
                                    val cb=context.getSystemService(android.content.ClipboardManager::class.java)
                                    cb.setPrimaryClip(android.content.ClipData.newPlainText("GitHub 授权码",challenge.userCode))
                                    androidx.browser.customtabs.CustomTabsIntent.Builder().build().launchUrl(context,android.net.Uri.parse(GitHubAuthorization.browserUrl))
                                    val grant=GitHubAuthorization.awaitGrant(challenge)
                                    status=withContext(Dispatchers.IO){repo.syncAuthorized(id,LoginInput.GitHubGrant(grant))}
                                } catch(e:CancellationException){throw e}
                                catch(e:Exception){message=e.message ?: "GitHub 登录未完成"}
                                finally{busy=false;githubCode=null;githubJob=null}
                            }
                        },enabled=!busy){Text(if(busy) "等待 GitHub 授权…" else "复制授权码并打开 GitHub")}
                        MotionTextButton({githubJob?.cancel();githubCode=null}){Text("取消登录")}
                    }
                }else{
                OutlinedTextField(value=gistToken,onValueChange={gistToken=it},singleLine=true,enabled=!busy,
                    visualTransformation=PasswordVisualTransformation(),label={Text("GitHub 访问令牌")})
                Text("使用有 Gist 读写权限的令牌。",style=MaterialTheme.typography.bodySmall)
                MotionButton({ val input=LoginInput.Token(gistToken); operate {repo.syncAuthorized(id,input)} },
                    enabled=!busy && gistToken.isNotBlank()){Text("绑定 GitHub 并同步")}
                }
                if(githubCode==null) MotionTextButton({useToken=!useToken;gistToken="";message=""},enabled=!busy){Text(if(useToken)"使用浏览器登录" else "使用访问令牌")}
            } else if (status?.connected != true) MotionButton({
                busy = true
                message = ""
                try {
                    val service = StorageOAuth.browser(context)
                    browser = service
                    pendingProvider=id
                    login.launch(service.getAuthorizationRequestIntent(repo.authorizationRequest(id)))
                } catch (_: Exception) {
                    browser = null; busy = false; message = "请安装支持 Custom Tabs 的浏览器后重试"
                }
            }, enabled = !busy && status != null) { Text(if(busy) "正在处理…" else "登录 ${id.title} 并同步") }
            else {
                status?.let { s ->
                    MaintenanceValue("上次同步",if(s.lastSync>0) java.text.DateFormat.getDateTimeInstance(java.text.DateFormat.SHORT,java.text.DateFormat.SHORT).format(java.util.Date(s.lastSync)) else "尚未完成")
                    if(s.lastSync>0) MaintenanceHint("${s.records} 条记录 · ${s.pending} 条待匹配")
                }
                androidx.compose.foundation.layout.FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                    MotionButton({ operate { repo.sync(id) } }, enabled = !busy) { Text(if (busy) "正在同步…" else "立即同步") }
                    MotionTextButton({ operate { repo.logout(id) } }, enabled = !busy) { Text("退出此设备") }
                }
            }
        } }

        status?.let { s ->
            if(s.metadataPending>0) item {
                MaintenanceStatus("${s.metadataPending} 部作品的来源信息待补全。${s.metadataError} 下次同步时将重试。",error=true)
            }
            if (s.connected) item {
                MaintenanceCard("自动同步") {
                    for ((label, on, update) in listOf(
                        Triple("回到应用时同步", s.autoPolicy.foreground, s.autoPolicy.copy(foreground = !s.autoPolicy.foreground)),
                        Triple("后台定期同步", s.autoPolicy.periodic, s.autoPolicy.copy(periodic = !s.autoPolicy.periodic))
                    )) ToggleRow(label,null,on,!busy) { operate { repo.setAutoPolicy(update,id) } }
                    if (s.autoPolicy.periodic) ChoiceMenu("同步间隔", s.autoPolicy.intervalMinutes.toString(),
                        listOf(15,30,60,180,360).map { it.toString() to "$it 分钟" }) { operate { repo.setAutoPolicy(s.autoPolicy.copy(intervalMinutes = it.toInt()),id) } }
                    if (s.autoError.isNotBlank()) Text(s.autoError, color = MaterialTheme.colorScheme.error)
                }
            }
            if(s.conflicts.isNotEmpty()) item {
                MaintenanceCard { MaintenanceRow("待处理的冲突","${s.conflicts.size} 条记录有多个版本") {
                    MotionTextButton({showConflicts=!showConflicts}) { Text(if(showConflicts)"收起" else "查看") }
                } }
            }
            if(showConflicts) for (c in s.conflicts) item(key=c.key) { MaintenanceCard {
                Text(c.label)
                for (r in c.choices) {
                    val value = r.optJSONObject("value")
                    val summary = when {
                        value?.has("items") == true -> "${value.optString("name")} · ${value.optJSONArray("items")?.length() ?: 0} 首"
                        value != null -> "${value.optLong("positionMs") / 1000} 秒 ${value.optString("note")} ${if(value.optBoolean("completed")) "已听完" else ""}"
                        r.isNull("value") -> "已删除"
                        r.optBoolean("value") -> "已收藏"
                        else -> "未收藏"
                    }
                    MotionOutlinedButton({ operate { repo.resolve(c.key,r.getString("device"),r.getLong("seq")) } }, enabled=!busy) {
                        Text("采用 ${r.getString("device").take(8)} · $summary")
                    }
                }
            } }
        }
    }
}
