

# Config env vars needed for hadoop

export JAVA_HOME=/usr/lib/jvm/jre-1.8.0-openjdk

export ZEROSIM_HADOOP_HOME=$HOME/0sim-workspace/bmks/zerosim-hadoop

export HADOOP_HOME=$ZEROSIM_HADOOP_HOME/hadoop
export HADOOP_CONF_DIR=$HADOOP_HOME/etc/hadoop
export HADOOP_MAPRED_HOME=$HADOOP_HOME
export HADOOP_COMMON_HOME=$HADOOP_HOME
export HADOOP_HDFS_HOME=$HADOOP_HOME
export YARN_HOME=$HADOOP_HOME

export SPARK_HOME=$ZEROSIM_HADOOP_HOME/spark

export HIBENCH_HOME=$ZEROSIM_HADOOP_HOME/HiBench
