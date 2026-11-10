package com.asmrplayer.android.library

import android.content.ContentResolver
import android.net.Uri
import android.os.CancellationSignal
import android.provider.DocumentsContract
import kotlinx.coroutines.*
import java.io.IOException

/** Enumerates only descendants of the granted tree. Never constructs filesystem paths. */
class SafTreeReader(private val resolver: ContentResolver) {
    suspend fun read(tree: Uri, onProgress: (Int) -> Unit = {}): Pair<String, List<ScannedDocument>> = withContext(Dispatchers.IO) {
        require(androidx.core.provider.DocumentsContractCompat.isTreeUri(tree))
        val docId = DocumentsContract.getTreeDocumentId(tree)
        val rootUri = DocumentsContract.buildDocumentUriUsingTree(tree, docId)
        val title = cancellableQuery { signal ->
            resolver.query(rootUri, arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null, signal)?.use {
                if (!it.moveToFirst()) throw java.io.FileNotFoundException("目录不存在")
                it.getString(0)
            } ?: throw java.io.FileNotFoundException("无法读取目录")
        }
        val pending = ArrayDeque<Pair<String, String>>()
        pending.add(docId to "")
        val seen = hashSetOf<String>()
        val files = mutableListOf<ScannedDocument>()
        while (pending.isNotEmpty()) {
            currentCoroutineContext().ensureActive()
            val (parent, prefix) = pending.removeFirst()
            if (!seen.add(parent)) throw IOException("提供者返回循环目录")
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parent)
            cancellableQuery { signal ->
                resolver.query(children, arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                    DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_MIME_TYPE,
                    DocumentsContract.Document.COLUMN_SIZE, DocumentsContract.Document.COLUMN_LAST_MODIFIED), null, null, null, signal)?.use { cursor ->
                    if (cursor.extras.getBoolean(DocumentsContract.EXTRA_LOADING) || cursor.extras.getString(DocumentsContract.EXTRA_ERROR) != null)
                        throw IOException("提供者尚未完成目录读取，请稍后重试")
                    while (cursor.moveToNext()) {
                        currentCoroutineContext().ensureActive()
                        val id = cursor.getString(0); val name = cursor.getString(1)
                        require(name.isNotBlank() && '/' !in name && name != "." && name != "..") { "无效文档名" }
                        val path = if (prefix.isEmpty()) name else "$prefix/$name"
                        if (cursor.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR) pending.add(id to path)
                        else files.add(ScannedDocument(path, DocumentsContract.buildDocumentUriUsingTree(tree, id).toString(), cursor.getLong(3), cursor.getLong(4)))
                        onProgress(files.size)
                    }
                } ?: throw IOException("目录读取失败，未提交扫描")
            }
        }
        title to files
    }

    private suspend fun <T> cancellableQuery(block: suspend (CancellationSignal) -> T): T = coroutineScope {
        val signal = CancellationSignal()
        // The watcher runs independently of a blocking Binder query and cancels its platform signal.
        val watcher = launch(Dispatchers.Default, start = CoroutineStart.UNDISPATCHED) {
            try { awaitCancellation() } finally { signal.cancel() }
        }
        try { block(signal) } finally { watcher.cancel() }
    }
}
