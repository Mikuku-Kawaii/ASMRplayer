package com.asmrplayer.android

import android.net.Uri
import android.os.Bundle
import android.provider.DocumentsContract
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.library.RootMode
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class LibraryToolsTest {
    @Test fun previewConfirmationCancellationAndVersionScopedSubtitles() = runBlocking {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val backend = AndroidBackend.get(context)
        val provider = Uri.parse("content://com.asmrplayer.android.test.fixture")
        val tree = DocumentsContract.buildTreeDocumentUri(provider.authority!!, "ui-tools-${System.nanoTime()}")
        context.contentResolver.call(provider, "mode", "normal", null)
        context.contentResolver.call(provider, "grant", context.packageName, Bundle().apply { putString("uri", tree.toString()) })
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        val tools = LibraryTools(context, scope)
        fun main(action: () -> Unit) = instrumentation.runOnMainSync(action)
        suspend fun state(predicate: (LibraryToolsState) -> Boolean) = withTimeout(10000) { tools.state.first(predicate) }
        try {
            main { tools.prepareRoot(tree, null) }
            state { it.preview != null }
            assertFalse(backend.library.roots.first().any { it.treeUri == tree.toString() })
            main { tools.preview(RootMode.WORK) }
            state { it.preview?.mode == RootMode.WORK && !it.previewBusy }
            main { tools.confirmImport() }
            val root = withTimeout(10000) { backend.library.roots.first { list -> list.any { it.treeUri == tree.toString() && it.lastScanMs > 0 } } }
                .single { it.treeUri == tree.toString() }
            val work = backend.library.works.first().single { it.rootId == root.id }
            val versions = backend.library.versions(work.id).first()
            val first = versions.single { it.groupPath == "WAV" && it.title == "01" }
            val second = versions.single { it.groupPath == "WAV" && it.title == "02" }
            main { tools.selectVersion(first.id, work.id) }
            val loaded = state { it.versionId == first.id && it.sources.isNotEmpty() && it.preference != null }
            main { tools.chooseSubtitle(first.id, false, loaded.sources.first().id) }
            state { !it.subtitleSaving && it.subtitles?.original != null }
            main { tools.saveOffsets(first.id, 500, -200) }
            val shifted = state { !it.subtitleSaving && it.subtitles?.preference?.offsetMs == 500L }
            assertTrue(shifted.subtitles!!.active(200).first.isEmpty())
            assertTrue(shifted.subtitles.active(600).first.isNotEmpty())
            main { tools.selectVersion(second.id, work.id) }
            state { it.versionId == second.id && !it.subtitleBusy }
            assertNull(tools.state.value.subtitles?.original)
            main { tools.chooseSubtitle(first.id, false, loaded.sources.first().id) }
            assertNotNull(tools.state.value.error)
            assertNull(backend.subtitles.preference(second.id).first()?.originalId)
            main { tools.selectVersion(first.id, work.id) }
            state { it.subtitles?.preference?.offsetMs == 500L }
            context.contentResolver.call(provider, "mode", "slow", null)
            main { tools.scan(root.id) }
            state { it.scans[root.id]?.running == true }
            main { tools.cancelScan(root.id) }
            state { it.scans[root.id]?.running == false }
            assertTrue(backend.library.versions(work.id).first().all { it.available })
        } finally {
            scope.cancel()
            context.contentResolver.call(provider, "mode", "normal", null)
            context.contentResolver.releasePersistableUriPermission(tree, android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
    }
}
