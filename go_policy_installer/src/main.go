package main

import (
	"encoding/json"
	"fmt"
	"os"
	"log"
	"os/exec"
	"path/filepath"
	"strings"
)

type ActionMap map[string]string

// LoadActionMap loads the JSON file that maps file/dir names to script commands.
func LoadActionMap(jsonPath string) (ActionMap, error) {
	data, err := os.ReadFile(jsonPath)
	if err != nil {
		return nil, fmt.Errorf("reading action json: %w", err)
	}
	var m ActionMap
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("unmarshal action json: %w", err)
	}
	return m, nil
}

// ProcessChangeset orchestrates everything.
//
// changeset  - the raw git changeset string
// policyDir  - path to vm-policies (or policy directory root)
// actions    - map of "name" -> "script command" from JSON
func ProcessChangeset(changeset, policyDir string, actions ActionMap) error {
	trimmed := strings.TrimSpace(changeset)

	if trimmed == "" {
		// 3) changeset empty → for each entry in JSON, if exists in policyDir, run script
		return runForExistingEntries(policyDir, actions)
	}

	// 1) get list of modified top-level names inside vm-policies (no recursive)
	names := parseTopLevelNames(changeset, "vm-policies")
	if len(names) == 0 {
		// nothing relevant changed under vm-policies
		return nil
	}

	// 2) for each name, locate action and run script with policyDir/name
	for name := range names {
		action, ok := actions[name]
		if !ok {
			// No action specified for this name, skip (or log)
			fmt.Fprintf(os.Stderr, "no action found for %q, skipping\n", name)
			continue
		}
		targetPath := filepath.Join(policyDir, name)
		if err := runActionScript(action, targetPath); err != nil {
			return fmt.Errorf("running action for %q: %w", name, err)
		}
	}

	return nil
}

// parseTopLevelNames parses the changeset and returns a set of top-level items
// under the given root (e.g., "vm-policies").
func parseTopLevelNames(changeset, root string) map[string]struct{} {
	result := make(map[string]struct{})
	lines := strings.Split(changeset, "\n")

	prefix := root + "/"

	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}

		// Expect format: "<status> <path>" e.g. "M vm-policies/hello.txt"
		parts := strings.Fields(line)
		if len(parts) < 2 {
			continue
		}
		path := parts[1]

		if !strings.HasPrefix(path, prefix) {
			continue
		}

		// Strip "vm-policies/"
		rel := strings.TrimPrefix(path, prefix)
		if rel == "" {
			continue
		}

		// Take only the first path component: no recursion
		top := strings.SplitN(rel, "/", 2)[0]
		if top != "" {
			result[top] = struct{}{}
		}
	}

	return result
}

// runForExistingEntries is used when the changeset is empty:
// For each key in the JSON, if policyDir/<name> exists, run its script.
func runForExistingEntries(policyDir string, actions ActionMap) error {
	for name, action := range actions {
		targetPath := filepath.Join(policyDir, name)
		if _, err := os.Stat(targetPath); err != nil {
			// doesn't exist, skip
			if os.IsNotExist(err) {
				continue
			}
			return fmt.Errorf("stat %q: %w", targetPath, err)
		}

		if err := runActionScript(action, targetPath); err != nil {
			return fmt.Errorf("running action for %q: %w", name, err)
		}
	}
	return nil
}

// runActionScript executes the given script command, appending targetPath as last argument.
//
// action string may be a simple command ("./script.sh") or include extra args
// ("bash ./script.sh -flag").
func runActionScript(action, targetPath string) error {
	action = strings.TrimSpace(action)
	if action == "" {
		return fmt.Errorf("empty action command")
	}

	parts := strings.Fields(action)
	cmdName := parts[0]
	args := append(parts[1:], targetPath)

	cmd := exec.Command(cmdName, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	return cmd.Run()
}

// Example usage (for reference):
//
 func main() {
    changeset := `
		 M vm-policies/hotplug.conf
		 A vm-policies/polset/newpolicy.conf
		`

     policyDir := "./testdata/vm-policies" // or wherever your vm-policies directory is
     actions, err := LoadActionMap("./actions.json")
     if err != nil {
         log.Fatalf("error loading actions: %v", err)
     }

     if err := ProcessChangeset(changeset, policyDir, actions); err != nil {
         log.Fatalf("error processing changeset: %v", err)
     }
 }
