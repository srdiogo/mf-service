// SPDX-License-Identifier: GPL-2.0-only
/* Copyright (c) 2026 Pavel Olizko <contact@nolight.dev>
 * Copyright (c) 2026 sleebu <contact@sleebu.dev>
 */
/* MiniFuse toggles and MiniFuse 4 status/verified mixer transactions.
 * Existing toggle interface: Pavel Olizko and sleebu (2026).
 * Mixer protocol and transaction implementation adapted from minifusecc.
 * See docs/mf4/evidence.md for provenance and validation boundaries.
 */
#include "mf4_protocol.h"
#include <linux/compat.h>
#include <linux/delay.h>
#include <linux/fs.h>
#include <linux/miscdevice.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <linux/usb.h>

#define MF4_VID 0x1c75
#define MF4_PID 0xaf70

struct mf4_record {
	u8 magic[4];
	__le16 version;
	__le16 vendor;
	__le16 product;
	__le16 bus;
	__le16 address;
	__le16 length;
	u8 state[MF4_STATE_SIZE];
} __packed;

struct mf4_search {
	struct usb_device *device;
	unsigned int count;
};

static DEFINE_MUTEX(mf4_query_mutex);

/* Byte-wise access works on all alignments without a version-specific
 * unaligned-access header dependency. The USB state is always little-endian.
 */
static u16 mf4_get_word(const u8 *data)
{
	return data[0] | ((u16)data[1] << 8);
}

static void mf4_put_word(u16 value, u8 *data)
{
	data[0] = value & 0xff;
	data[1] = value >> 8;
}

static int mf4_status(struct usb_device *device, u8 *data)
{
	int result = usb_control_msg(device, usb_rcvctrlpipe(device, 0), 0xb2,
				     USB_DIR_IN | USB_TYPE_CLASS | USB_RECIP_INTERFACE, 0, 0, data,
				     MF4_STATE_SIZE, 750);
	return result < 0 ? result : result == MF4_STATE_SIZE ? 0 : -EPROTO;
}

static int mf4_set(struct usb_device *device, u16 selector, __le16 *data)
{
	int result = usb_control_msg(device, usb_sndctrlpipe(device, 0), 0x22,
				     USB_TYPE_CLASS | USB_RECIP_INTERFACE, selector, 0, data,
				     sizeof(*data), 750);
	return result < 0 ? result : result == sizeof(*data) ? 0 : -EPROTO;
}

struct mf4_work {
	struct mf4_transaction tx;
	u8 before[MF4_STATE_SIZE];
	u8 predicted[MF4_STATE_SIZE];
	__le16 originals[MF4_MAX_CHANGES];
};

static int mf4_find(struct usb_device *device, void *context)
{
	struct mf4_search *search = context;

	if (le16_to_cpu(device->descriptor.idVendor) != MF4_VID ||
	    le16_to_cpu(device->descriptor.idProduct) != MF4_PID)
		return 0;
	search->count++;
	if (!search->device)
		search->device = usb_get_dev(device);
	return 0;
}

static int mf4_wait_state(struct usb_device *device, struct mf4_transaction *tx, const u8 *expected)
{
	int result, attempt;
	u16 flags = le16_to_cpu(tx->flags) & ~MF4_TX_AFTER_VALID;
	tx->flags = cpu_to_le16(flags);
	for (attempt = 0; attempt < 5; attempt++) {
		msleep(20);
		result = mf4_status(device, tx->after);
		if (result)
			return result;
		tx->flags = cpu_to_le16(flags | MF4_TX_AFTER_VALID);
		if (!memcmp(tx->after, expected, MF4_STATE_SIZE))
			return 0;
	}
	return -EIO;
}

/* Fixed, read-only C2 query. No request fields or selectors come from userspace. */
static long mf4_meters(struct file *file, unsigned long arg)
{
	struct mf4_search search = {};
	struct mf4_meter_record *record;
	int result;

	if (!(file->f_mode & FMODE_READ))
		return -EACCES;
	record = kzalloc(sizeof(*record), GFP_KERNEL);
	if (!record)
		return -ENOMEM;
	if (mutex_lock_interruptible(&mf4_query_mutex)) {
		result = -ERESTARTSYS;
		goto free;
	}
	usb_for_each_dev(&search, mf4_find);
	if (!search.device || search.count != 1) {
		result = search.device ? -EBUSY : -ENODEV;
		goto unlock;
	}
	memcpy(record->magic, "MF4M", 4);
	record->version = cpu_to_le16(1);
	record->vendor = cpu_to_le16(MF4_VID);
	record->product = cpu_to_le16(MF4_PID);
	record->bus = cpu_to_le16(search.device->bus->busnum);
	record->address = cpu_to_le16(search.device->devnum);
	record->length = cpu_to_le16(MF4_METER_SIZE);
	result = usb_control_msg(search.device, usb_rcvctrlpipe(search.device, 0), 0xc2,
				 USB_DIR_IN | USB_TYPE_CLASS | USB_RECIP_INTERFACE,
				 0, 0, record->data, MF4_METER_SIZE, 750);
	if (result >= 0)
		result = result == MF4_METER_SIZE ? 0 : -EPROTO;
	if (!result && copy_to_user((void __user *)arg, record, sizeof(*record)))
		result = -EFAULT;
unlock:
	if (search.device)
		usb_put_dev(search.device);
	mutex_unlock(&mf4_query_mutex);
free:
	kfree(record);
	return result;
}

static long mf4_apply(struct file *file, unsigned int command, unsigned long arg)
{
	struct mf4_work *work;
	struct mf4_transaction *tx;
	struct mf4_search search = {};
	u16 count, offset, mask, value, word, selector, flags;
	unsigned int i, j, attempted = 0;
	int result, failure, undo_result = 0;

	if (command == MF4_IOCTL_CAPABILITIES) {
		const struct mf4_capabilities capabilities = {
			.magic = { 'M', 'F', '4', 'C' },
			.version = cpu_to_le16(1),
			.length = cpu_to_le16(sizeof(struct mf4_capabilities)),
			.flags = cpu_to_le32(MF4_CAP_INPUT_SWITCHES | MF4_CAP_METER_QUERY),
		};
		if (!(file->f_mode & FMODE_READ))
			return -EACCES;
		return copy_to_user((void __user *)arg, &capabilities, sizeof(capabilities)) ? -EFAULT : 0;
	}
	if (command == MF4_IOCTL_METERS)
		return mf4_meters(file, arg);
	if (command != MF4_IOCTL_APPLY)
		return -ENOTTY;
	if (!(file->f_mode & FMODE_WRITE))
		return -EACCES;
	work = kzalloc(sizeof(*work), GFP_KERNEL);
	if (!work)
		return -ENOMEM;
	tx = &work->tx;
	if (copy_from_user(tx, (void __user *)arg, sizeof(*tx))) {
		result = -EFAULT;
		goto release;
	}
	tx->completed = 0;
	tx->flags = 0;
	memset(tx->after, 0, MF4_STATE_SIZE);
	count = le16_to_cpu(tx->count);
	if (le16_to_cpu(tx->version) != MF4_TX_VERSION || !count || count > MF4_MAX_CHANGES) {
		result = -EINVAL;
		goto reply;
	}
	/* Validate the ENTIRE batch before even considering a hardware write. */
	for (i = 0; i < count; i++) {
		selector = le16_to_cpu(tx->changes[i].selector);
		value = le16_to_cpu(tx->changes[i].value);
		result = mf4_target(selector, value, &offset, &mask);
		if (result)
			goto reply;
		for (j = 0; j < i; j++) {
			if (tx->changes[j].selector == tx->changes[i].selector) {
				result = -EINVAL;
				goto reply;
			}
		}
	}
	if (mutex_lock_interruptible(&mf4_query_mutex)) {
		result = -ERESTARTSYS;
		goto reply;
	}
	usb_for_each_dev(&search, mf4_find);
	if (!search.device || search.count != 1) {
		result = search.device ? -EBUSY : -ENODEV;
		goto unlock;
	}
	if (search.device->bus->busnum != le16_to_cpu(tx->bus) ||
	    search.device->devnum != le16_to_cpu(tx->address)) {
		result = -ESTALE;
		goto unlock;
	}
	result = mf4_status(search.device, work->before);
	if (result)
		goto unlock;
	memcpy(tx->after, work->before, MF4_STATE_SIZE);
	tx->flags = cpu_to_le16(MF4_TX_AFTER_VALID);
	if (memcmp(tx->expected, work->before, MF4_STATE_SIZE)) {
		result = -EAGAIN;
		goto unlock;
	}
	memcpy(work->predicted, work->before, MF4_STATE_SIZE);
	for (i = 0; i < count; i++) {
		selector = le16_to_cpu(tx->changes[i].selector);
		value = le16_to_cpu(tx->changes[i].value);
		result = mf4_target(selector, value, &offset, &mask);
		if (result)
			goto unlock;
		word = mf4_get_word(work->before + offset);
		work->originals[i] = cpu_to_le16(mask ? !!(word & mask) : word);
		/* Refuse a change if its exact original cannot be restored. */
		result = mf4_target(selector, le16_to_cpu(work->originals[i]), &offset, &mask);
		if (result)
			goto unlock;
		word = mf4_get_word(work->predicted + offset);
		word = mask ? ((word & ~mask) | (value ? mask : 0)) : value;
		mf4_put_word(word, work->predicted + offset);
	}
	if (!memcmp(work->before, work->predicted, MF4_STATE_SIZE)) {
		result = 0;
		goto unlock;
	}
	tx->flags = 0;
	for (i = 0; i < count; i++) {
		attempted = i + 1;
		result = mf4_set(search.device, le16_to_cpu(tx->changes[i].selector),
				 &tx->changes[i].value);
		if (result)
			goto rollback;
		tx->completed = cpu_to_le16(i + 1);
		msleep(10);
	}
	result = mf4_wait_state(search.device, tx, work->predicted);
	if (!result)
		goto unlock;
rollback:
	failure = result;
	flags = le16_to_cpu(tx->flags) | MF4_TX_ROLLBACK_ATTEMPTED;
	tx->flags = cpu_to_le16(flags);
	/* Include the failed transfer: a transport error does not prove that
	 * firmware did not apply that command. Restore all attempted selectors.
	 */
	for (j = attempted; j > 0; j--) {
		i = j - 1;
		result = mf4_set(search.device, le16_to_cpu(tx->changes[i].selector),
				 &work->originals[i]);
		if (result)
			undo_result = result;
		msleep(10);
	}
	result = mf4_wait_state(search.device, tx, work->before);
	if (!result) {
		tx->flags = cpu_to_le16(le16_to_cpu(tx->flags) | MF4_TX_ROLLBACK_VERIFIED);
		result = failure;
	} else {
		result = -EREMOTEIO;
	}
	/* A matching full readback is stronger than an OUT acknowledgement. */
	(void)undo_result;
unlock:
	if (search.device)
		usb_put_dev(search.device);
	mutex_unlock(&mf4_query_mutex);
reply:
	if (copy_to_user((void __user *)arg, tx, sizeof(*tx)))
		result = -EFAULT;
release:
	kfree(work);
	return result;
}

static ssize_t mf4_read(struct file *file, char __user *buffer, size_t count, loff_t *position)
{
	struct mf4_search search = {};
	struct mf4_record *record;
	int result;

	if (*position)
		return 0;
	if (count < sizeof(*record))
		return -EMSGSIZE;
	if (mutex_lock_interruptible(&mf4_query_mutex))
		return -ERESTARTSYS;

	usb_for_each_dev(&search, mf4_find);
	if (!search.device) {
		result = -ENODEV;
		goto unlock;
	}
	if (search.count != 1) {
		result = -EBUSY;
		goto put;
	}
	record = kzalloc(sizeof(*record), GFP_KERNEL);
	if (!record) {
		result = -ENOMEM;
		goto put;
	}

	memcpy(record->magic, "MF4S", 4);
	record->version = cpu_to_le16(1);
	record->vendor = cpu_to_le16(MF4_VID);
	record->product = cpu_to_le16(MF4_PID);
	record->bus = cpu_to_le16(search.device->bus->busnum);
	record->address = cpu_to_le16(search.device->devnum);
	record->length = cpu_to_le16(MF4_STATE_SIZE);

	result = mf4_status(search.device, record->state);
	if (result)
		goto free;
	if (copy_to_user(buffer, record, sizeof(*record))) {
		result = -EFAULT;
		goto free;
	}
	*position += sizeof(*record);
	result = sizeof(*record);
free:
	kfree(record);
put:
	usb_put_dev(search.device);
unlock:
	mutex_unlock(&mf4_query_mutex);
	return result;
}

struct mf_search {
	struct usb_device *device;
	unsigned int count;
};

static int mf_find_supported(struct usb_device *device, void *context)
{
	struct mf_search *search = context;
	u16 pid = le16_to_cpu(device->descriptor.idProduct);
	if (le16_to_cpu(device->descriptor.idVendor) != MF4_VID ||
	    (pid != 0xaf70 && pid != 0xaf80 && pid != 0xaf82 && pid != 0xaf90))
		return 0;
	search->count++;
	if (!search->device)
		search->device = usb_get_dev(device);
	return 0;
}

static ssize_t mf_legacy_write(struct file *file, const char __user *buffer, size_t count,
			       loff_t *position)
{
	struct mf_search search = {};
	char text[32], trailing;
	unsigned int selector;
	int value, result;
	__le16 *payload;
	u16 pid;

	if (!count || count >= sizeof(text))
		return -EINVAL;
	if (copy_from_user(text, buffer, count))
		return -EFAULT;
	text[count] = '\0';
	if (sscanf(text, "%x %d %c", &selector, &value, &trailing) != 2 || selector > 0xffff ||
	    (value != 0 && value != 1))
		return -EINVAL;
	if (mutex_lock_interruptible(&mf4_query_mutex))
		return -ERESTARTSYS;
	usb_for_each_dev(&search, mf_find_supported);
	if (!search.device || search.count != 1) {
		result = search.device ? -EBUSY : -ENODEV;
		goto unlock;
	}
	pid = le16_to_cpu(search.device->descriptor.idProduct);
	if (!mf_legacy_allowed(pid, selector)) {
		result = -EOPNOTSUPP;
		goto unlock;
	}
	payload = kmalloc(sizeof(*payload), GFP_KERNEL);
	if (!payload) {
		result = -ENOMEM;
		goto unlock;
	}
	*payload = cpu_to_le16(value);
	result = mf4_set(search.device, selector, payload);
	kfree(payload);
	if (!result)
		result = count;
unlock:
	if (search.device)
		usb_put_dev(search.device);
	mutex_unlock(&mf4_query_mutex);
	return result;
}

static const struct file_operations mf4_operations = {
    .owner = THIS_MODULE,
    .open = nonseekable_open,
    .read = mf4_read,
    .write = mf_legacy_write,
    .unlocked_ioctl = mf4_apply,
#ifdef CONFIG_COMPAT
    .compat_ioctl = compat_ptr_ioctl,
#endif
};

static struct miscdevice mf4_misc = {
    .minor = MISC_DYNAMIC_MINOR,
    .name = "minifuse_cmd",
    .fops = &mf4_operations,
    .mode = 0660,
};

static int __init mf4_init(void)
{
	BUILD_BUG_ON(sizeof(struct mf4_record) != 206);
	BUILD_BUG_ON(sizeof(struct mf4_transaction) != 1032);
	return misc_register(&mf4_misc);
}

static void __exit mf4_exit(void)
{
	misc_deregister(&mf4_misc);
}

module_init(mf4_init);
module_exit(mf4_exit);
MODULE_LICENSE("GPL");
MODULE_AUTHOR("Pavel Olizko <contact@nolight.dev>");
MODULE_AUTHOR("sleebu <contact@sleebu.dev>");
MODULE_DESCRIPTION("Arturia MiniFuse toggles and verified MiniFuse 4 mixer transactions");
MODULE_VERSION("0.2.6");
