/* SPDX-License-Identifier: GPL-2.0-only */
#include "../../../kmod/mf4_protocol.h"
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

static uint64_t fold(uint64_t hash, unsigned int value)
{
	return (hash ^ value) * UINT64_C(1099511628211);
}

int main(void)
{
	static const __u16 values[] = {0, 1,	2,    3,      4,      5,      6,
				       7, 3072, 3073, 0x8000, 0xc000, 0xbfff, 0xffff};
	uint64_t hash = UINT64_C(14695981039346656037);
	unsigned int selector, i;
	__u16 offset, mask;

	_Static_assert(sizeof(struct mf4_transaction) == 1032, "transaction ABI size");
	_Static_assert(offsetof(struct mf4_transaction, after) == 838, "state offset");
	_Static_assert(offsetof(struct mf4_transaction, flags) == 1030, "flags offset");
	_Static_assert(sizeof(struct mf4_capabilities) == 16, "capabilities ABI size");
	_Static_assert(sizeof(struct mf4_meter_record) == 80, "meter record ABI size");
	_Static_assert(offsetof(struct mf4_meter_record, data) == 16, "meter data offset");
	_Static_assert(offsetof(struct mf4_capabilities, flags) == 8, "capability flags offset");
	assert(mf_legacy_allowed(0xaf80, 0x0400));
	assert(mf_legacy_allowed(0xaf82, 0x0500));
	assert(mf_legacy_allowed(0xaf90, 0x0400));
	assert(!mf_legacy_allowed(0xaf90, 0x0300));
	assert(!mf_legacy_allowed(0xaf90, 0x0001));
	assert(mf_legacy_allowed(0xaf70, 0x0300));
	assert(mf_legacy_allowed(0xaf70, 0x0001));
	assert(!mf_legacy_allowed(0xaf70, 0x0400));
	assert(!mf_legacy_allowed(0xaf70, 0x0500));
	assert(!mf_legacy_allowed(0x1234, 0));
	for (selector = 0; selector <= 0xffff; selector++) {
		for (i = 0; i < sizeof(values) / sizeof(values[0]); i++) {
			int accepted = mf4_target(selector, values[i], &offset, &mask) == 0;
			hash = fold(hash, accepted);
			if (accepted) {
				hash = fold(hash, offset);
				hash = fold(hash, mask);
			}
		}
	}
	printf("%lu %llu %lu %lu\n", (unsigned long)MF4_IOCTL_APPLY,
	       (unsigned long long)hash, (unsigned long)MF4_IOCTL_CAPABILITIES,
	       (unsigned long)MF4_IOCTL_METERS);
	return 0;
}
