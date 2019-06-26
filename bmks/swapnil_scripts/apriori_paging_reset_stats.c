#include<linux/unistd.h>
#include<stdio.h>
#include<stdlib.h>
#include<unistd.h>
#include<sys/syscall.h>

#define __NR_order_count 332

int main(int argc,char* argv[])
{
	syscall(__NR_order_count, getpid(), 1);
	return 0;
}
