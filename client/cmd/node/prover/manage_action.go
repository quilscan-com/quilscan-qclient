package prover

import (
	"context"
	"encoding/hex"
	"fmt"
	"strings"
	"time"

	"source.quilibrium.com/quilibrium/monorepo/protobufs"
)

var (
	manageAction        string
	manageActionFilters []string
	manageActionWorkers []uint
	manageWait          bool
	manageWaitTimeout   time.Duration
)

func init() {
	NodeProverManageCmd.Flags().StringVar(
		&manageAction,
		"action",
		"",
		"run a non-interactive action: join, leave, confirm, reject, pause, resume, manual, auto",
	)
	NodeProverManageCmd.Flags().StringArrayVar(
		&manageActionFilters,
		"filter",
		nil,
		"filter to act on; may be repeated; positional filters are also accepted",
	)
	NodeProverManageCmd.Flags().UintSliceVar(
		&manageActionWorkers,
		"worker",
		nil,
		"worker core id for manual/auto mode actions; may be repeated or comma-separated",
	)
	NodeProverManageCmd.Flags().BoolVar(
		&manageWait,
		"wait",
		false,
		"wait until the selected filter status changes after broadcasting",
	)
	NodeProverManageCmd.Flags().DurationVar(
		&manageWaitTimeout,
		"wait-timeout",
		2*time.Minute,
		"maximum time to wait for --wait",
	)
}

type manageActionPlan struct {
	action           string
	filters          [][]byte
	originalStatuses []uint32
	workers          []uint32
	manual           bool
}

func runManageAction(
	client protobufs.NodeServiceClient,
	m manageModel,
	args []string,
) error {
	filterArgs := append([]string(nil), manageActionFilters...)
	filterArgs = append(filterArgs, args...)

	plan, err := buildManageActionPlan(m, manageAction, filterArgs, manageActionWorkers)
	if err != nil {
		return err
	}

	switch plan.action {
	case "Manual", "Auto":
		if err := runManageModeAction(client, plan); err != nil {
			return err
		}
		fmt.Printf("%s mode set for %d worker(s)\n", strings.ToLower(plan.action), len(plan.workers))
		return nil
	case "Join":
		msg := doJoin(client, plan.filters)()
		res, ok := msg.(actionResultMsg)
		if !ok {
			return fmt.Errorf("join returned unexpected response %T", msg)
		}
		if res.err != nil {
			return res.err
		}
		fmt.Printf("Join request submitted for %d filter(s)\n", len(plan.filters))
	default:
		if err := runManageBroadcastAction(client, plan); err != nil {
			return err
		}
	}

	if manageWait {
		return waitForManageAction(client, plan)
	}
	return nil
}

func buildManageActionPlan(
	m manageModel,
	action string,
	filterArgs []string,
	workerArgs []uint,
) (manageActionPlan, error) {
	normalizedAction := strings.ToLower(strings.TrimSpace(action))
	switch normalizedAction {
	case "manual", "auto":
		workers, err := normalizeWorkerIDs(workerArgs)
		if err != nil {
			return manageActionPlan{}, err
		}
		if len(workers) == 0 {
			return manageActionPlan{}, fmt.Errorf("--worker is required for %s action", normalizedAction)
		}
		return manageActionPlan{
			action:  titleAction(normalizedAction),
			workers: workers,
			manual:  normalizedAction == "manual",
		}, nil
	case "join":
		filters, _, err := selectAvailableFilters(m, filterArgs)
		if err != nil {
			return manageActionPlan{}, err
		}
		return manageActionPlan{action: "Join", filters: filters}, nil
	case "leave", "confirm", "reject", "pause", "resume":
		return buildAllocationActionPlan(m, normalizedAction, filterArgs)
	default:
		return manageActionPlan{}, fmt.Errorf("unsupported manage action %q", action)
	}
}

func buildAllocationActionPlan(
	m manageModel,
	action string,
	filterArgs []string,
) (manageActionPlan, error) {
	rows, err := selectAllocationRows(m, filterArgs)
	if err != nil {
		return manageActionPlan{}, err
	}

	plan := manageActionPlan{
		action:           titleAction(action),
		filters:          make([][]byte, 0, len(rows)),
		originalStatuses: make([]uint32, 0, len(rows)),
	}
	for _, row := range rows {
		if err := validateAllocationAction(m, action, row); err != nil {
			return manageActionPlan{}, err
		}
		plan.filters = append(plan.filters, row.filter)
		plan.originalStatuses = append(plan.originalStatuses, row.status)
	}
	return plan, nil
}

func selectAvailableFilters(m manageModel, filterArgs []string) ([][]byte, []shardRow, error) {
	if len(filterArgs) == 0 {
		return nil, nil, fmt.Errorf("at least one --filter or positional filter is required")
	}

	byFilter := make(map[string]shardRow, len(m.available))
	for _, row := range m.available {
		byFilter[row.filterKey] = row
		byFilter[row.filterHex] = row
	}

	filters := make([][]byte, 0, len(filterArgs))
	rows := make([]shardRow, 0, len(filterArgs))
	for _, arg := range filterArgs {
		key, raw, err := normalizeFilterArg(arg)
		if err != nil {
			return nil, nil, err
		}
		row, ok := byFilter[key]
		if !ok {
			return nil, nil, fmt.Errorf("filter %s is not available to join", key)
		}
		if len(row.filter) > 0 {
			raw = row.filter
		}
		filters = append(filters, raw)
		rows = append(rows, row)
	}
	return filters, rows, nil
}

func selectAllocationRows(m manageModel, filterArgs []string) ([]allocationRow, error) {
	if len(filterArgs) == 0 {
		return nil, fmt.Errorf("at least one --filter or positional filter is required")
	}

	byFilter := make(map[string]allocationRow, len(m.allocations))
	for _, row := range m.allocations {
		if row.filterKey != "" {
			byFilter[row.filterKey] = row
		}
		if row.filterHex != "" {
			byFilter[row.filterHex] = row
		}
	}

	rows := make([]allocationRow, 0, len(filterArgs))
	for _, arg := range filterArgs {
		key, raw, err := normalizeFilterArg(arg)
		if err != nil {
			return nil, err
		}
		row, ok := byFilter[key]
		if !ok {
			return nil, fmt.Errorf("filter %s is not allocated on this prover", key)
		}
		if len(row.filter) == 0 {
			row.filter = raw
		}
		rows = append(rows, row)
	}
	return rows, nil
}

func validateAllocationAction(m manageModel, action string, row allocationRow) error {
	status := row.status
	statusName := allocationStatusLabel(row)

	switch action {
	case "leave":
		if status != 2 {
			return fmt.Errorf("leave requires Active status; filter %s is %s", row.filterHex, statusName)
		}
	case "confirm":
		if status != 1 && status != 4 {
			return fmt.Errorf("confirm requires Joining or Leaving status; filter %s is %s", row.filterHex, statusName)
		}
		openFrame, closeFrame, ok := confirmWindow(row)
		if !ok {
			return fmt.Errorf("confirm window unavailable for filter %s", row.filterHex)
		}
		if m.frameNumber < openFrame || m.frameNumber >= closeFrame {
			return fmt.Errorf(
				"confirm not available for filter %s at frame %d; window is [%d, %d)",
				row.filterHex,
				m.frameNumber,
				openFrame,
				closeFrame,
			)
		}
	case "reject":
		if status != 1 && status != 4 {
			return fmt.Errorf("reject requires Joining or Leaving status; filter %s is %s", row.filterHex, statusName)
		}
	case "pause":
		if status != 2 {
			return fmt.Errorf("pause requires Active status; filter %s is %s", row.filterHex, statusName)
		}
	case "resume":
		if status != 3 {
			return fmt.Errorf("resume requires Paused status; filter %s is %s", row.filterHex, statusName)
		}
	}
	return nil
}

func runManageBroadcastAction(client protobufs.NodeServiceClient, plan manageActionPlan) error {
	if len(plan.filters) == 0 {
		return fmt.Errorf("no filters selected")
	}

	switch plan.action {
	case "Leave", "Confirm", "Reject":
		prepared, err := prepareManageMultiFilterAction(client, plan)
		if err != nil {
			return err
		}
		broadcast, err := broadcastPreparedManageAction(client, prepared)
		if err != nil {
			return err
		}
		fmt.Printf("%s broadcast at frame %d for %d filter(s)\n", plan.action, broadcast.sendFrame, len(plan.filters))
	case "Pause", "Resume":
		for i, filter := range plan.filters {
			single := manageActionPlan{
				action:           plan.action,
				filters:          [][]byte{filter},
				originalStatuses: []uint32{plan.originalStatuses[i]},
			}
			prepared, err := prepareManageSingleFilterAction(client, single)
			if err != nil {
				return err
			}
			broadcast, err := broadcastPreparedManageAction(client, prepared)
			if err != nil {
				return err
			}
			fmt.Printf("%s broadcast at frame %d for %s\n", plan.action, broadcast.sendFrame, hex.EncodeToString(filter))
		}
	default:
		return fmt.Errorf("unsupported broadcast action %s", plan.action)
	}

	return nil
}

func prepareManageMultiFilterAction(
	client protobufs.NodeServiceClient,
	plan manageActionPlan,
) (actionPreparedMsg, error) {
	originalStatus := uint32(0)
	if len(plan.originalStatuses) > 0 {
		originalStatus = plan.originalStatuses[0]
	}

	var msg any
	switch plan.action {
	case "Leave":
		msg = doLeave(client, plan.filters, originalStatus)()
	case "Confirm":
		msg = doConfirm(client, plan.filters, originalStatus)()
	case "Reject":
		msg = doReject(client, plan.filters, originalStatus)()
	default:
		return actionPreparedMsg{}, fmt.Errorf("unsupported multi-filter action %s", plan.action)
	}
	prepared, ok := msg.(actionPreparedMsg)
	if !ok {
		return actionPreparedMsg{}, fmt.Errorf("%s returned unexpected response %T", plan.action, msg)
	}
	if prepared.err != nil {
		return actionPreparedMsg{}, prepared.err
	}
	return prepared, nil
}

func prepareManageSingleFilterAction(
	client protobufs.NodeServiceClient,
	plan manageActionPlan,
) (actionPreparedMsg, error) {
	originalStatus := uint32(0)
	if len(plan.originalStatuses) > 0 {
		originalStatus = plan.originalStatuses[0]
	}

	var msg any
	switch plan.action {
	case "Pause":
		msg = doPause(client, plan.filters[0], originalStatus)()
	case "Resume":
		msg = doResume(client, plan.filters[0], originalStatus)()
	default:
		return actionPreparedMsg{}, fmt.Errorf("unsupported single-filter action %s", plan.action)
	}
	prepared, ok := msg.(actionPreparedMsg)
	if !ok {
		return actionPreparedMsg{}, fmt.Errorf("%s returned unexpected response %T", plan.action, msg)
	}
	if prepared.err != nil {
		return actionPreparedMsg{}, prepared.err
	}
	return prepared, nil
}

func broadcastPreparedManageAction(
	client protobufs.NodeServiceClient,
	prepared actionPreparedMsg,
) (actionBroadcastMsg, error) {
	msg := sendAction(client, prepared)()
	broadcast, ok := msg.(actionBroadcastMsg)
	if !ok {
		return actionBroadcastMsg{}, fmt.Errorf("%s broadcast returned unexpected response %T", prepared.action, msg)
	}
	if broadcast.err != nil {
		return actionBroadcastMsg{}, broadcast.err
	}
	return broadcast, nil
}

func runManageModeAction(client protobufs.NodeServiceClient, plan manageActionPlan) error {
	for _, worker := range plan.workers {
		ctx, cancel := withTimeout()
		_, err := client.SetManuallyManaged(
			ctx,
			&protobufs.SetManuallyManagedRequest{
				CoreId:          worker,
				ManuallyManaged: plan.manual,
			},
		)
		cancel()
		if err != nil {
			return fmt.Errorf("worker %d: %w", worker, err)
		}
	}
	return nil
}

func waitForManageAction(client protobufs.NodeServiceClient, plan manageActionPlan) error {
	if len(plan.filters) == 0 {
		return nil
	}

	if manageWaitTimeout <= 0 {
		return fmt.Errorf("--wait-timeout must be positive")
	}

	deadline := time.Now().Add(manageWaitTimeout)
	if plan.action == "Join" {
		return waitForJoinInclusion(client, plan, deadline)
	}

	entries := make([]awaitFilterEntry, len(plan.filters))
	for i, filter := range plan.filters {
		originalStatus := uint32(0)
		if i < len(plan.originalStatuses) {
			originalStatus = plan.originalStatuses[i]
		}
		entries[i] = awaitFilterEntry{filter: filter, originalStatus: originalStatus}
	}

	for {
		msg := checkAllocationStatus(client, plan.action, entries)()
		result, ok := msg.(awaitResultMsg)
		if !ok {
			return fmt.Errorf("wait returned unexpected response %T", msg)
		}
		if result.err != nil {
			return result.err
		}

		allSettled := true
		for _, outcome := range result.perFilter {
			if !outcome.settled {
				allSettled = false
				break
			}
		}
		if allSettled {
			for _, outcome := range result.perFilter {
				fmt.Printf(
					"%s settled: %s -> %s\n",
					plan.action,
					hex.EncodeToString(outcome.filter),
					outcome.outcome,
				)
			}
			return nil
		}

		if time.Now().After(deadline) {
			return fmt.Errorf("%s wait timed out after %s", plan.action, manageWaitTimeout)
		}
		time.Sleep(3 * time.Second)
	}
}

func waitForJoinInclusion(
	client protobufs.NodeServiceClient,
	plan manageActionPlan,
	deadline time.Time,
) error {
	for {
		ctx, cancel := context.WithTimeout(context.Background(), rpcTimeout)
		nodeInfo, err := client.GetNodeInfo(ctx, &protobufs.GetNodeInfoRequest{})
		cancel()
		if err != nil {
			return err
		}

		present := make(map[string]uint32, len(nodeInfo.GetShardAllocations()))
		for _, allocation := range nodeInfo.GetShardAllocations() {
			present[hex.EncodeToString(allocation.GetFilter())] = allocation.GetStatus()
		}

		allPresent := true
		for _, filter := range plan.filters {
			key := hex.EncodeToString(filter)
			status, ok := present[key]
			if !ok {
				allPresent = false
				break
			}
			statusName, ok := allocationStatusNames[status]
			if !ok {
				statusName = fmt.Sprintf("Unknown(%d)", status)
			}
			fmt.Printf("Join observed: %s -> %s\n", key, statusName)
		}
		if allPresent {
			return nil
		}

		if time.Now().After(deadline) {
			return fmt.Errorf("Join wait timed out after %s", manageWaitTimeout)
		}
		time.Sleep(3 * time.Second)
	}
}

func normalizeFilterArg(arg string) (string, []byte, error) {
	arg = strings.TrimSpace(arg)
	if arg == "" {
		return "", nil, fmt.Errorf("empty filter")
	}
	raw, err := hex.DecodeString(arg)
	if err != nil {
		return "", nil, fmt.Errorf("invalid filter hex %q: %w", arg, err)
	}
	return hex.EncodeToString(raw), raw, nil
}

func normalizeWorkerIDs(ids []uint) ([]uint32, error) {
	workers := make([]uint32, 0, len(ids))
	for _, id := range ids {
		if id > uint(^uint32(0)) {
			return nil, fmt.Errorf("worker id %d overflows uint32", id)
		}
		workers = append(workers, uint32(id))
	}
	return workers, nil
}

func confirmWindow(row allocationRow) (uint64, uint64, bool) {
	switch row.status {
	case 1:
		if row.joinFrame == 0 {
			return 0, 0, false
		}
		return row.joinFrame + ACTION_FRAME_DELAY, row.joinFrame + ACTION_FRAME_DELAY*2, true
	case 4:
		if row.leaveFrame == 0 {
			return 0, 0, false
		}
		return row.leaveFrame + ACTION_FRAME_DELAY, row.leaveFrame + ACTION_FRAME_DELAY*2, true
	default:
		return 0, 0, false
	}
}

func allocationStatusLabel(row allocationRow) string {
	if row.statusName != "" {
		return row.statusName
	}
	if name, ok := allocationStatusNames[row.status]; ok {
		return name
	}
	return fmt.Sprintf("Unknown(%d)", row.status)
}

func titleAction(action string) string {
	switch action {
	case "join":
		return "Join"
	case "leave":
		return "Leave"
	case "confirm":
		return "Confirm"
	case "reject":
		return "Reject"
	case "pause":
		return "Pause"
	case "resume":
		return "Resume"
	case "manual":
		return "Manual"
	case "auto":
		return "Auto"
	default:
		return action
	}
}
