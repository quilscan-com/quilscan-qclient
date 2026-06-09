package prover

import (
	"encoding/hex"
	"math/big"
	"strings"
	"testing"
)

func TestBuildManageActionPlanValidatesPanelAndStatus(t *testing.T) {
	const leavingFilter = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	const activeFilter = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	const availableFilter = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

	m := newManageModel(nil)
	m.frameNumber = 1000
	m.allocations = []allocationRow{
		{
			filterHex:       leavingFilter,
			filterKey:       leavingFilter,
			filter:          mustDecodeHexForTest(t, leavingFilter),
			status:          4,
			statusName:      "Leaving",
			leaveFrame:      600,
			shardSize:       big.NewInt(0),
			estimatedReward: big.NewInt(0),
		},
		{
			filterHex:       activeFilter,
			filterKey:       activeFilter,
			filter:          mustDecodeHexForTest(t, activeFilter),
			status:          2,
			statusName:      "Active",
			shardSize:       big.NewInt(0),
			estimatedReward: big.NewInt(0),
		},
	}
	m.available = []shardRow{
		{
			filterHex:       availableFilter,
			filterKey:       availableFilter,
			filter:          mustDecodeHexForTest(t, availableFilter),
			shardSize:       big.NewInt(0),
			estimatedReward: big.NewInt(0),
		},
	}

	rejectPlan, err := buildManageActionPlan(m, "reject", []string{leavingFilter}, nil)
	if err != nil {
		t.Fatalf("reject plan failed: %v", err)
	}
	if rejectPlan.action != "Reject" || len(rejectPlan.filters) != 1 || rejectPlan.originalStatuses[0] != 4 {
		t.Fatalf("unexpected reject plan: %+v", rejectPlan)
	}

	joinPlan, err := buildManageActionPlan(m, "join", []string{availableFilter}, nil)
	if err != nil {
		t.Fatalf("join plan failed: %v", err)
	}
	if joinPlan.action != "Join" || len(joinPlan.filters) != 1 {
		t.Fatalf("unexpected join plan: %+v", joinPlan)
	}

	if _, err := buildManageActionPlan(m, "leave", []string{leavingFilter}, nil); err == nil ||
		!strings.Contains(err.Error(), "Leaving") {
		t.Fatalf("expected leave on leaving allocation to fail with status context, got %v", err)
	}

	if _, err := buildManageActionPlan(m, "join", []string{activeFilter}, nil); err == nil ||
		!strings.Contains(err.Error(), "not available") {
		t.Fatalf("expected join on allocated filter to fail, got %v", err)
	}
}

func TestBuildManageActionPlanManualModeUsesWorkers(t *testing.T) {
	m := newManageModel(nil)

	plan, err := buildManageActionPlan(m, "manual", nil, []uint{7, 8})
	if err != nil {
		t.Fatalf("manual plan failed: %v", err)
	}
	if plan.action != "Manual" || len(plan.workers) != 2 || !plan.manual {
		t.Fatalf("unexpected manual plan: %+v", plan)
	}

	plan, err = buildManageActionPlan(m, "auto", nil, []uint{7})
	if err != nil {
		t.Fatalf("auto plan failed: %v", err)
	}
	if plan.action != "Auto" || len(plan.workers) != 1 || plan.manual {
		t.Fatalf("unexpected auto plan: %+v", plan)
	}
}

func mustDecodeHexForTest(t *testing.T, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatalf("decode hex %q: %v", s, err)
	}
	return b
}
