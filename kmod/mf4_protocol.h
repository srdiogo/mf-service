/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef MF_CLI_PROTOCOL_H
#define MF_CLI_PROTOCOL_H

#ifdef __KERNEL__
#include <linux/errno.h>
#include <linux/ioctl.h>
#else
#include <errno.h>
#include <sys/ioctl.h>
#endif
#include <linux/types.h>

#define MF4_STATE_SIZE 190
#define MF4_MAX_CHANGES 160
#define MF4_TX_VERSION 1
#define MF4_TX_ROLLBACK_ATTEMPTED 1
#define MF4_TX_ROLLBACK_VERIFIED 2
#define MF4_TX_AFTER_VALID 4

struct mf4_change {
	__le16 selector;
	__le16 value;
} __attribute__((packed));

struct mf4_transaction {
	__le16 version;
	__le16 bus;
	__le16 address;
	__le16 count;
	__u8 expected[MF4_STATE_SIZE];
	struct mf4_change changes[MF4_MAX_CHANGES];
	__u8 after[MF4_STATE_SIZE];
	__le16 completed;
	__le16 flags;
} __attribute__((packed));

/* Keep the pre-existing MF1/MF2 selector map; MF2 per-input INST is an
 * independent upstream change, not inferred from the MF4 protocol. */
static inline int mf_legacy_allowed(__u16 pid, __u16 selector)
{
	if (pid == 0xaf70)
		return selector == 0x0000 || selector == 0x0001 || selector == 0x0300;
	if (pid == 0xaf80 || pid == 0xaf82 || pid == 0xaf90)
		return selector == 0x0000 || selector == 0x0400 || selector == 0x0500;
	return 0;
}

#define MF4_CAP_INPUT_SWITCHES 1
#define MF4_CAP_METER_QUERY 2
#define MF4_METER_SIZE 64
struct mf4_meter_record {
	__u8 magic[4];
	__le16 version;
	__le16 vendor;
	__le16 product;
	__le16 bus;
	__le16 address;
	__le16 length;
	__u8 data[MF4_METER_SIZE];
} __attribute__((packed));
#define MF4_IOCTL_METERS _IOR('M', 0x43, struct mf4_meter_record)
struct mf4_capabilities {
	__u8 magic[4];
	__le16 version;
	__le16 length;
	__le32 flags;
	__le32 reserved;
} __attribute__((packed));
#define MF4_IOCTL_CAPABILITIES _IOR('M', 0x42, struct mf4_capabilities)

#define MF4_IOCTL_APPLY _IOWR('M', 0x41, struct mf4_transaction)
/* Return the B2 word affected by a captured selector, and its optional bit.
 * The three input switches have model-specific, physically validated masks.
 * Unknown features, clocks, firmware and LED controls remain excluded.
 */
static inline int mf4_target(__u16 selector, __u16 value, __u16 *offset, __u16 *mask)
{
	unsigned int feature = selector >> 8, channel = selector & 0xff;
	unsigned int mix;
	__s16 level = (__s16)value;
	*mask = 0;
	switch (feature) {
	case 0x00:
		if (channel > 1 || value > 1)
			return -EINVAL;
		*offset = 0;
		*mask = 1U << channel;
		return 0;
	case 0x03:
		if (channel || value > 1)
			return -EINVAL;
		*offset = 6;
		*mask = 1;
		return 0;
	case 0x09:
	case 0x13:
	case 0x1d:
	case 0x27:
	case 0x31:
	case 0x3b:
		if (level > 12 * 256)
			return -ERANGE;
		switch (feature) {
		case 0x09:
			*offset = 18;
			break;
		case 0x13:
			*offset = 38;
			break;
		case 0x1d:
			*offset = 58;
			break;
		case 0x27:
			*offset = 78;
			break;
		case 0x31:
			*offset = 98;
			break;
		default:
			*offset = 118;
			break;
		}
		if (channel >= (feature < 0x31 ? 8 : 10))
			return -EINVAL;
		*offset += 2 * channel;
		return 0;
	case 0x45:
	case 0x46:
	case 0x47:
		if (channel || level > 0 || (level < -64 * 256 && level != -32768))
			return -ERANGE;
		*offset = 138 + 2 * (feature - 0x45);
		return 0;
	case 0x4c:
	case 0x4d:
	case 0x4e:
		mix = feature - 0x4c;
		*offset = 152 + 2 * mix;
		break;
	case 0x4f:
	case 0x50:
	case 0x51:
		mix = feature - 0x4f;
		*offset = 158 + 2 * mix;
		break;
	case 0x52:
	case 0x53:
	case 0x54:
		/* Mono insertion is exposed only for the four analog inputs. */
		mix = feature - 0x52;
		if (channel >= 4)
			return -EINVAL;
		*offset = 164 + 2 * mix;
		break;
	case 0x55:
	case 0x57:
		/* 0x56 stereo writes are intentionally not enabled: activation
		 * on outputs 3/4 has not been isolated in the reference captures.
		 */
		mix = feature == 0x55 ? 0 : 2;
		if (channel >= (mix == 0 ? 4 : 5))
			return -EINVAL;
		*offset = 170 + 2 * mix;
		break;
	case 0x58:
	case 0x59:
	case 0x5a:
		if (channel)
			return -EINVAL;
		if ((feature == 0x58 && value > 1) ||
		    (feature == 0x59 && (value < 2 || value > 3)) ||
		    (feature == 0x5a && (value < 4 || value > 7)))
			return -EINVAL;
		*offset = 176 + 2 * (feature - 0x58);
		return 0;
	default:
		return -EOPNOTSUPP;
	}
	if (value > 1 || channel >= (mix == 2 ? 10 : 8))
		return -EINVAL;
	*mask = (__u16)(1U << channel);
	return 0;
}

#endif
