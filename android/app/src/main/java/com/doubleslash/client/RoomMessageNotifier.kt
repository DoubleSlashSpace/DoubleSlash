package com.doubleslash.client

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent

/**
 * A message alert for one room, shown when that room's alerts are on and the
 * app is not in front.
 *
 * One notification per room, replaced by the next message in that room.
 * Opening the room cancels it. Ids stay clear of the foreground-service
 * notification (1) and the incoming-call notification (2).
 */
object RoomMessageNotifier {

    const val ACTION_OPEN = "com.doubleslash.client.ROOM_MESSAGE_OPEN"
    const val EXTRA_ROOM_ID = "room_id"
    const val EXTRA_SUPERNODE_ID = "supernode_id"

    private const val CHANNEL_ID = "doubleslash_room_messages"

    private val activeRoomIds = LinkedHashSet<String>()

    fun show(
        context: Context,
        roomId: String,
        supernodeId: String,
        roomName: String,
        sender: String,
        body: String,
        unread: Int,
    ) {
        if (roomId.isEmpty()) return
        val app = context.applicationContext
        val manager = app.getSystemService(NotificationManager::class.java) ?: return
        ensureChannel(app, manager)
        activeRoomIds.add(roomId)
        val title = roomName.ifBlank { "Room" }
        val who = sender.ifBlank { "Someone" }
        val preview = body.ifBlank { "New message" }.let { text ->
            if (text.length <= 80) text else text.take(79) + "\u2026"
        }
        val notification = Notification.Builder(app, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText("$who: $preview")
            .setCategory(Notification.CATEGORY_MESSAGE)
            .setAutoCancel(true)
            .setContentIntent(openIntent(app, roomId, supernodeId))
            .setNumber(unread.coerceAtLeast(1))
            .build()
        manager.notify(notificationId(roomId), notification)
    }

    fun cancel(context: Context, roomId: String) {
        if (roomId.isEmpty()) return
        activeRoomIds.remove(roomId)
        context.applicationContext
            .getSystemService(NotificationManager::class.java)
            ?.cancel(notificationId(roomId))
    }

    fun cancelAll(context: Context) {
        val ids = activeRoomIds.toList()
        activeRoomIds.clear()
        val manager = context.applicationContext
            .getSystemService(NotificationManager::class.java) ?: return
        ids.forEach { manager.cancel(notificationId(it)) }
    }

    /** Stable id above the service and call notifications. */
    internal fun notificationId(roomId: String): Int {
        val mixed = roomId.hashCode() and 0x000FFFFF
        return 0x1000 + mixed
    }

    private fun openIntent(context: Context, roomId: String, supernodeId: String): PendingIntent {
        val intent = Intent(context, MainActivity::class.java).apply {
            action = ACTION_OPEN
            putExtra(EXTRA_ROOM_ID, roomId)
            putExtra(EXTRA_SUPERNODE_ID, supernodeId)
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or
                Intent.FLAG_ACTIVITY_CLEAR_TOP or
                Intent.FLAG_ACTIVITY_NEW_TASK
        }
        return PendingIntent.getActivity(
            context,
            notificationId(roomId),
            intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    private fun ensureChannel(context: Context, manager: NotificationManager) {
        if (manager.getNotificationChannel(CHANNEL_ID) != null) return
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                context.getString(R.string.room_message_channel),
                NotificationManager.IMPORTANCE_HIGH,
            ).apply {
                description = context.getString(R.string.room_message_channel_description)
            },
        )
    }
}
