package prover

import (
	"fmt"
	"math/big"
	"strings"
	"text/tabwriter"
)

var manageOnce bool

func init() {
	NodeProverManageCmd.Flags().BoolVar(
		&manageOnce,
		"once",
		false,
		"print current manage tables once and exit",
	)
}

func renderManageOncePlain(m manageModel) string {
	var b strings.Builder

	fmt.Fprintf(&b, "Peer ID: %s\n", m.peerId)
	if m.frameNumber > 0 {
		fmt.Fprintf(&b, "Frame: %d\n", m.frameNumber)
	}
	if m.difficulty > 0 {
		fmt.Fprintf(&b, "Difficulty: %d\n", m.difficulty)
	}
	fmt.Fprintf(&b, "Running Workers: %d\n", m.runningWorkers)
	fmt.Fprintf(&b, "Allocated Workers: %d\n", m.allocatedWorkers)
	fmt.Fprintf(&b, "Free Workers: %s\n", formatWorkerList(m.freeWorkers))
	fmt.Fprintf(&b, "Reachable: %v\n", m.reachable)

	fmt.Fprintf(&b, "\nAllocations (%d):\n", len(m.allocations))
	if len(m.allocations) == 0 {
		b.WriteString("  No allocations\n")
	} else {
		tw := tabwriter.NewWriter(&b, 0, 0, 2, ' ', 0)
		fmt.Fprintln(tw, "Select\tFilter\tProvers\tRing\tSize [MB]\tShards\tReward [Q/f]\tWorker\tStatus\tMode\tNext Action\tDefault Action")
		for _, a := range m.sortedAllocations() {
			mode := ""
			if a.manuallyManaged {
				mode = "M"
			}
			fmt.Fprintf(
				tw,
				"[ ]\t%s\t%d\t%d\t%s\t%d\t~%s\t%s\t%s\t%s\t%s\t%s\n",
				a.filterHex,
				a.activeProvers,
				a.ring,
				formatPlainMB(a.shardSize),
				a.dataShards,
				formatQUIL(nonNilBig(a.estimatedReward)),
				formatWorkerID(a.workerID),
				a.statusName,
				mode,
				a.nextAction,
				a.defaultAction,
			)
		}
		_ = tw.Flush()
	}

	fmt.Fprintf(&b, "\nAvailable Shards (%d):\n", len(m.available))
	if len(m.available) == 0 {
		b.WriteString("  No available shards\n")
	} else {
		tw := tabwriter.NewWriter(&b, 0, 0, 2, ' ', 0)
		fmt.Fprintln(tw, "Select\tFilter\tProvers\tRing\tSize [MB]\tShards\tReward [Q/f]")
		for _, s := range m.sortedAvailable() {
			fmt.Fprintf(
				tw,
				"[ ]\t%s\t%d\t%d\t%s\t%d\t~%s\n",
				s.filterHex,
				s.activeProvers,
				s.ring,
				formatPlainMB(s.shardSize),
				s.dataShards,
				formatQUIL(nonNilBig(s.estimatedReward)),
			)
		}
		_ = tw.Flush()
	}

	return b.String()
}

func formatPlainMB(v *big.Int) string {
	if v == nil {
		return "0.0"
	}
	return fmt.Sprintf("%.1f", float64(v.Uint64())/float64(1024*1024))
}

func formatWorkerID(id int) string {
	if id < 0 {
		return "-"
	}
	return fmt.Sprintf("%d", id)
}

func formatWorkerList(ids []uint32) string {
	if len(ids) == 0 {
		return "-"
	}
	parts := make([]string, len(ids))
	for i, id := range ids {
		parts[i] = fmt.Sprintf("%d", id)
	}
	return strings.Join(parts, ",")
}

func nonNilBig(v *big.Int) *big.Int {
	if v == nil {
		return big.NewInt(0)
	}
	return v
}
