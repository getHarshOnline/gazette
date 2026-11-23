// Package fixture describes an owned, synthetic HTTP interface.
//
// This fixture is documentation evidence, not a running service.
//
// Schemes: https
// Host: api.example.invalid
// BasePath: /v1
// Version: 0.1.0
//
// Consumes:
// - application/json
//
// Produces:
// - application/json
//
//	SecurityDefinitions:
//	apiKey:
//	  type: apiKey
//	  name: X-Fixture-Key
//	  in: header
//
// swagger:meta
package fixture

// Widget is a synthetic response object.
//
// swagger:model Widget
type Widget struct {
	// Stable widget identifier.
	// required: true
	ID string `json:"id"`
	// Human-readable label.
	Name string `json:"name"`
}

// WidgetResponse contains one widget.
//
// swagger:response WidgetResponse
type WidgetResponse struct {
	// in: body
	Body Widget
}

// WidgetParameters selects a widget and optional details.
//
// swagger:parameters getWidget
type WidgetParameters struct {
	// Exact widget key.
	// in: path
	// required: true
	ID string `json:"id"`
	// Include extended details.
	// in: query
	Verbose bool `json:"verbose"`
}

// GetWidget swagger:route GET /widgets/{id} widgets getWidget
//
// Read one widget.
//
// Security:
// - apiKey:
//
// Responses:
// 200: WidgetResponse
func GetWidget() {}
