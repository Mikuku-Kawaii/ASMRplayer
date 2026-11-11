package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.asmrplayer.android.PlayerViewModel
import com.asmrplayer.android.sync.SyncStatus
import androidx.media3.common.util.UnstableApi

@Composable
internal fun SyncResultDetails(status: SyncStatus) {
    val summary=status.details.optJSONArray("summary") ?: return
    MaintenanceCard("同步结果") {
        MaintenanceHint("当前同步资料；本次接收含新增、更改和删除。待匹配记录已保留，尚未关联音源。")
        for(index in 0 until summary.length() step 2) Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(12.dp)) {
            for(i in index..minOf(index+1,summary.length()-1)) {
                val row=summary.getJSONObject(i)
                Column(Modifier.weight(1f).padding(vertical=6.dp),verticalArrangement=Arrangement.spacedBy(3.dp)) {
                    Text(row.getString("label"),style=MaterialTheme.typography.labelMedium)
                    Text(row.optInt("total").toString(),style=MaterialTheme.typography.titleLarge)
                    MaintenanceHint("本次接收 ${row.optInt("received")}"+if(row.optInt("pending")>0) " · 待匹配 ${row.optInt("pending")}" else "")
                }
            }
        }
        if(status.pending>0) DetailDisclosure("待匹配内容","${status.pending} 条记录已保留") {
            MaintenanceHint("导入相同音频或补全在线作品后再次同步。不同剪辑版本不自动合并。")
            val pending=status.details.optJSONArray("pendingItems")
            for(i in 0 until (pending?.length() ?: 0)) {
                val row=pending!!.getJSONObject(i)
                Text(row.getString("label"),style=MaterialTheme.typography.bodyMedium)
                MaintenanceHint(row.getString("reason"))
            }
            if(status.pending>(pending?.length() ?: 0)) MaintenanceHint("显示前 ${pending?.length() ?: 0} 条；其他记录及歌单缺失音轨仍保留。")
        }
    }
}

@UnstableApi
@Composable
internal fun SyncResumeCard(status:SyncStatus,model:PlayerViewModel) {
    val resume=status.resume ?: return
    val state by model.screenState.collectAsState()
    val work=state.lookup.forVersion(resume.getString("versionId"))
    val hidden=state.privateMetadata || work?.let { !visibleRating(it.rating,state) }==true
    MaintenanceCard("继续另一台设备的收听") {
        Text(if(hidden) "内容已隐藏" else resume.optString("title"),maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.titleSmall)
        val seconds=resume.getLong("positionMs")/1000
        MaintenanceHint("${resume.optString("deviceName")} · ${seconds/60}:${(seconds%60).toString().padStart(2,'0')}")
        MaintenanceHint(java.text.DateFormat.getDateTimeInstance(java.text.DateFormat.SHORT,java.text.DateFormat.SHORT).format(java.util.Date(resume.getLong("updatedMs"))))
        Row(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
            MotionButton({model.resumeSynced(resume.getString("id"),status.provider)},enabled=!state.opening) {Text("继续收听")}
            MotionTextButton({model.dismissSynced(resume.getString("id"),status.provider)},enabled=!state.opening) {Text("忽略此次")}
        }
    }
}
