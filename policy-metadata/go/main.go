package main

import (
	"encoding/json"
	"fmt"
)

// PolicyMetaData is our "Class" structure
type PolicyMetaData struct {
	// We use a map to hold dynamic JSON fields
	data map[string]interface{}
}

// NewPolicyMetaData is our "Constructor"
func NewPolicyMetaData(metadata string) (*PolicyMetaData, error) {
	p := &PolicyMetaData{
		data: make(map[string]interface{}),
	}

	// Unmarshal the input string into our map
	err := json.Unmarshal([]byte(metadata), &p.data)
	if err != nil {
		return nil, fmt.Errorf("failed to parse metadata: %v", err)
	}

	// Ensure the "type" field exists as per your requirement
	if _, ok := p.data["type"]; !ok {
		return nil, fmt.Errorf("metadata missing required field: 'type'")
	}

	return p, nil
}

// GetField is an "Instance Method"
// (p *PolicyMetaData) is the receiver, similar to 'this' or 'self'
func (p *PolicyMetaData) GetField(name string) string {
	val, ok := p.data[name]
	if !ok {
		return ""
	}

	// Safely convert the interface value to a string
	str, ok := val.(string)
	if !ok {
		// If it's not a string (like a number), return it as a string anyway
		return fmt.Sprintf("%v", val)
	}
	return str
}

func main() {
	// Usage
	rawJson := `{"type": "firewall_rule", "file_name": "rule1.json", "level": "high"}`
	
	meta, err := NewPolicyMetaData(rawJson)
	if err != nil {
		panic(err)
	}

	fmt.Println("Type:", meta.GetField("type"))   // Output: firewall_rule
	fmt.Println("Level:", meta.GetField("level")) // Output: high
	fmt.Println("File:", meta.GetField("file_name")) // Output: high
}
