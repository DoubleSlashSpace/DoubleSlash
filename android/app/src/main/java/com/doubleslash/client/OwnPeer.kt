package com.doubleslash.client

/**
 * Our own row, pinned above the Peers list like the desktop's.
 *
 * We are not a contact of ourselves, so a peer-store record of our own identity
 * stays out of the list's online and offline peers; the pinned row opens
 * self-chat, which reaches every device signed in as us.
 */
internal fun Peer.isOwnIdentity(identity: IdentityInfo): Boolean =
    (identity.peerId.isNotEmpty() && peerId == identity.peerId) ||
        (identity.publicId.isNotEmpty() && identityPub.isNotEmpty() &&
            identityPub.trimEnd('=') == identity.publicId.trimEnd('='))

/** The pinned row's status line, worded as on the desktop. */
internal fun ownStatusLabel(online: Boolean, otherDevices: Int): String = when {
    !online -> "Offline"
    otherDevices <= 0 -> "Online · this device only"
    otherDevices == 1 -> "Online · 1 other device"
    else -> "Online · $otherDevices other devices"
}
