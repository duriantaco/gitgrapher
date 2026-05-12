package sample

import "fmt"

type Embedded struct {
	ID string
}

type User struct {
	Embedded
	NameValue string
}

type Repository interface {
	Save(user User) error
}

func NewUser(name string) User {
	fmt.Println(name)
	return User{NameValue: name}
}

func (u User) Name() string {
	return u.NameValue
}
