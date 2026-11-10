package com.asmrplayer.android

import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.incoming.IncomingTools
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class IncomingToolsTest {
    @Test fun sharedCopiesSurviveSourceRevocationAndSubtitleSelectionIsExplicit() = runBlocking {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest")) { "Use the isolated test application" }
        val backend=AndroidBackend.get(context)
        val provider=Uri.parse("content://${InstrumentationRegistry.getArguments().getString("fixtureAuthority", "com.asmrplayer.android.test.fixture")}")
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Main.immediate)
        val tools=IncomingTools(context,scope) {}
        suspend fun waitReady()=withTimeout(10_000) { tools.state.first { !it.busy } }
        try {
            context.contentResolver.call(provider,"mode","normal",null)
            val before=backend.library.works.first().map { it.id }.toSet()
            val audio=DocumentsContract.buildDocumentUri(provider.authority!!,"share/WAV/01.wav")
            withContext(Dispatchers.Main) { tools.receive(Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM,audio)) }
            assertEquals(1,waitReady().files.size)
            assertEquals(before,backend.library.works.first().map { it.id }.toSet())
            withContext(Dispatchers.Main) { tools.importFiles(null,false) }
            assertFalse(waitReady().visible)
            val work=backend.library.works.first().first { it.id !in before }
            val version=backend.library.versions(work.id).first().single().id
            val subtitle=DocumentsContract.buildDocumentUri(provider.authority!!,"share/WAV/01.ja.srt")
            withContext(Dispatchers.Main) { tools.receive(Intent(Intent.ACTION_VIEW,subtitle)) }
            assertTrue(waitReady().files.single().subtitle)
            withContext(Dispatchers.Main) { tools.importFiles(version,false) }
            assertFalse(waitReady().visible)
            context.contentResolver.call(provider,"mode","denied",null)
            val resolved=backend.library.resolve(version)
            assertEquals("${context.packageName}.imports",Uri.parse(resolved.uri).authority)
            assertTrue(context.contentResolver.openInputStream(Uri.parse(resolved.uri))!!.use { it.read() >= 0 })
            assertTrue(backend.subtitles.load(version).original!!.cues.isNotEmpty())
            val needs=backend.library.reauthorizationNeeds()
            assertFalse(needs.audioSourceIds.contains(resolved.sourceId))
            val subtitleIds=backend.subtitles.sources(version).first().map { it.id }
            assertTrue(needs.subtitleSourceIds.none { it in subtitleIds })
        } finally {
            withContext(Dispatchers.Main) { tools.dismiss() }
            scope.cancel()
            context.contentResolver.call(provider,"mode","normal",null)
        }
    }
}
