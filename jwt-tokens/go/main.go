package main

import (
	"fmt"
	"jwt-tokens/go/pb" // Changed import path
	"log"
	"os"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"google.golang.org/protobuf/proto"
)

func validateToken(tokenString string, secret string) (*pb.TokenClaims, error) {
	token, err := jwt.Parse(tokenString, func(token *jwt.Token) (interface{}, error) {
		if _, ok := token.Method.(*jwt.SigningMethodHMAC); !ok {
			return nil, fmt.Errorf("unexpected signing method: %v", token.Header["alg"])
		}
		return []byte(secret), nil
	})

	if err != nil {
		return nil, err
	}

	if claims, ok := token.Claims.(jwt.MapClaims); ok && token.Valid {
		data, ok := claims["data"].(string)
		if !ok {
			return nil, fmt.Errorf("data not found")
		}
		decodedData, err := jwt.Decode(data)
		if err != nil {
			return nil, err
		}
		var tokenClaims pb.TokenClaims
		err = proto.Unmarshal(decodedData, &tokenClaims)
		if err != nil {
			return nil, err
		}
		return &tokenClaims, nil
	} else {
		return nil, fmt.Errorf("invalid token")
	}
}

func main() {
	if len(os.Args) != 3 {
		fmt.Println("Usage: go run main.go <secret_key> <token_file>")
		os.Exit(1)
	}
	secret := os.Args[1]
	tokenFile := os.Args[2]

	// Read the token from the file
	tokenBytes, err := os.ReadFile(tokenFile)
	if err != nil {
		log.Fatalf("Failed to read token from file: %v", err)
	}
	tokenString := string(tokenBytes)

	claims, err := validateToken(tokenString, secret)
	if err != nil {
		log.Fatalf("Invalid Token: %v", err)
	}

	fmt.Printf("Valid Token! Claims: %+v\n", claims)
	fmt.Printf("Valid Token! Claims: %+v\n", claims.SrcVm)
	fmt.Printf("Valid Token! Claims: %+v\n", claims.DstVm)
	fmt.Printf("Valid Token! Claims: %+v\n", time.Unix(int64(claims.Iat), 0))
	fmt.Printf("Valid Token! Claims: %+v\n", time.Unix(int64(claims.Exp), 0))
}
