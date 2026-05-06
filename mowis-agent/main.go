package main

import (
	"github.com/mowisai/mowis-agent/cmd"
	"github.com/mowisai/mowis-agent/internal/logging"
)

func main() {
	defer logging.RecoverPanic("main", func() {
		logging.ErrorPersist("Application terminated due to unhandled panic")
	})

	cmd.Execute()
}
