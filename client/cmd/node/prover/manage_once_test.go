package prover

import (
	"math/big"
	"strings"
	"testing"
)

func TestRenderManageOncePlainIncludesAllocationAndAvailablePanels(t *testing.T) {
	m := newManageModel(nil)
	m.peerId = "peer-a"
	m.frameNumber = 651589
	m.runningWorkers = 8
	m.allocatedWorkers = 7
	m.reachable = true
	m.freeWorkers = []uint32{8}
	m.allocations = []allocationRow{
		{
			filterHex:       "11558584af7017a9bfd1ff1864302d643fbe58c62dcf90cbcd8fde74a26794d91c12",
			activeProvers:   7,
			ring:            0,
			shardSize:       big.NewInt(47 * 1024 * 1024),
			dataShards:      0,
			estimatedReward: big.NewInt(69720),
			workerID:        7,
			statusName:      "Leaving",
			manuallyManaged: true,
			nextAction:      "Reject@now | Confirm@651659",
			defaultAction:   "Confirm@652019",
		},
	}
	m.available = []shardRow{
		{
			filterHex:       "22558584af7017a9bfd1ff1864302d643fbe58c62dcf90cbcd8fde74a26794d91c12",
			activeProvers:   2,
			ring:            1,
			shardSize:       big.NewInt(12 * 1024 * 1024),
			dataShards:      4,
			estimatedReward: big.NewInt(12345678),
		},
	}

	out := renderManageOncePlain(m)

	for _, want := range []string{
		"Peer ID: peer-a",
		"Frame: 651589",
		"Free Workers: 8",
		"Allocations (1)",
		"11558584af7017a9bfd1ff1864302d643fbe58c62dcf90cbcd8fde74a26794d91c12",
		"Leaving",
		"M",
		"Reject@now | Confirm@651659",
		"Confirm@652019",
		"Available Shards (1)",
		"22558584af7017a9bfd1ff1864302d643fbe58c62dcf90cbcd8fde74a26794d91c12",
		"~0.12345678",
	} {
		if !strings.Contains(out, want) {
			t.Fatalf("rendered output missing %q:\n%s", want, out)
		}
	}
}
