// Run from Controls: go run /path/to/fabric/tests/host/main.go /path/to/fabric
// Creates private PostgreSQL and a workflow-free runtime, then runs Rust tests.
package main

import (
	"context"
	"fmt"
	embeddedpostgres "github.com/fergusstrange/embedded-postgres"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"time"
)

func run() error {
	if len(os.Args) != 2 {
		return fmt.Errorf("usage: go run host/main.go FABRIC_DIRECTORY")
	}
	root, err := os.MkdirTemp("/tmp", "fabric-test-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(root)
	binary := filepath.Join(root, "factory-runtime")
	build := exec.Command("go", "build", "-o", binary, "./cmd/factory-runtime")
	build.Stderr = os.Stderr
	if err = build.Run(); err != nil {
		return err
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return err
	}
	port := uint32(listener.Addr().(*net.TCPAddr).Port)
	listener.Close()
	db := embeddedpostgres.NewDatabase(embeddedpostgres.DefaultConfig().Port(port).Database("fabric_test").RuntimePath(filepath.Join(root, "pg")).DataPath(filepath.Join(root, "data")).Logger(io.Discard))
	if err = db.Start(); err != nil {
		return err
	}
	defer db.Stop()
	runtime := filepath.Join(root, "host")
	host := exec.Command(binary, "--root", runtime)
	host.Env = append(os.Environ(), fmt.Sprintf("FACTORY_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:%d/fabric_test?sslmode=disable", port), "FACTORY_HATCHET_MODE=invalid-no-engine")
	host.Stderr = os.Stderr
	if err = host.Start(); err != nil {
		return err
	}
	defer func() {
		host.Process.Signal(syscall.SIGTERM)
		done := make(chan error, 1)
		go func() { done <- host.Wait() }()
		select {
		case <-done:
		case <-time.After(15 * time.Second):
			host.Process.Kill()
			<-done
		}
	}()
	socket := filepath.Join(runtime, "factory.sock")
	client := &http.Client{Timeout: time.Second, Transport: &http.Transport{DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
		return (&net.Dialer{}).DialContext(ctx, "unix", socket)
	}}}
	defer client.CloseIdleConnections()
	ready := false
	for deadline := time.Now().Add(20 * time.Second); time.Now().Before(deadline); {
		response, e := client.Get("http://local/health")
		if e == nil {
			response.Body.Close()
			if response.StatusCode == 200 {
				ready = true
				break
			}
		}
		time.Sleep(50 * time.Millisecond)
	}
	if !ready {
		return fmt.Errorf("standalone host did not become ready")
	}
	test := exec.Command("cargo", "test", "--test", "runtime", "--", "--ignored", "--test-threads=1")
	test.Dir = os.Args[1]
	controls, err := os.Getwd()
	if err != nil {
		return err
	}
	test.Env = append(os.Environ(), "FABRIC_TEST_SOCKET="+socket, "FABRIC_TEST_CONTROLS="+controls)
	test.Stdout = os.Stdout
	test.Stderr = os.Stderr
	return test.Run()
}
func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
